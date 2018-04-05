use std::collections::HashMap;
use std::convert::AsRef;
use std::borrow::Borrow;
use std::str::FromStr;
use std::fmt::{self, Display};
use std::error::Error;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::ops::Deref;

use ascii::{IgnoreAsciiCaseStr, IgnoreAsciiCaseString};

//TODO potentially move this to common
pub struct EhloData {
    domain: Domain,
    data: HashMap<Capability, Vec<EhloParam>>
}

impl EhloData {

    pub fn new(domain: Domain, data: HashMap<Capability, Vec<EhloParam>>) -> Self {
        EhloData { domain, data }
    }

    pub fn has_capability<A>(&self, cap: A) -> bool
        where A: AsRef<str>
    {
        self.data.contains_key(<&IgnoreAsciiCaseStr>::from(cap.as_ref()))
    }

    pub fn get_capability_params<A>(&self, cap: A) -> Option<&[EhloParam]>
        where A: AsRef<str>
    {
        self.data.get(<&IgnoreAsciiCaseStr>::from(cap.as_ref()))
            .map(|vec| &**vec)
    }

    pub fn capability_map(&self) -> &HashMap<Capability, Vec<EhloParam>> {
        &self.data
    }

    pub fn domain(&self) -> &Domain {
        &self.domain
    }

}


impl Into<(Domain, HashMap<Capability, Vec<EhloParam>>)> for EhloData {
    fn into(self) -> (Domain, HashMap<Capability, Vec<EhloParam>>) {
        let EhloData { domain, data } = self;
        (domain, data)
    }
}


#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Capability(EsmtpKeyword);

impl Deref for Capability {
    type Target = EsmtpKeyword;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Borrow<IgnoreAsciiCaseStr> for Capability {

    fn borrow(&self) -> &IgnoreAsciiCaseStr {
        (self.0).0.as_ref()
    }
}

impl From<EsmtpKeyword> for Capability {
    fn from(keyword: EsmtpKeyword) -> Self {
        Capability(keyword)
    }
}

impl Into<EsmtpKeyword> for Capability {
    fn into(self) -> EsmtpKeyword {
        self.0
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct EsmtpKeyword(IgnoreAsciiCaseString);

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct EsmtpValue(String);

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct EhloParam(String);

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Domain(IgnoreAsciiCaseString);

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct AddressLiteral(IgnoreAsciiCaseString);

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ForwardPath(String);

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ReversePath(Option<String>);

macro_rules! impl_str_wrapper {
    ($($name:ident),*) => ($(

        impl $name {
            pub fn as_str(&self) -> &str {
                self.0.as_ref()
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.0.as_ref()
            }
        }

        impl Into<String> for $name {
            fn into(self) -> String {
                self.0.into()
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl<'a> PartialEq<&'a str> for $name {
            fn eq(&self, other: &&'a str) -> bool {
                self.0 == *other
            }
        }

        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                &self.0 == other
            }
        }

    )*);
}


impl_str_wrapper!(Domain, EhloParam, AddressLiteral, EsmtpKeyword, EsmtpValue);

impl ForwardPath {

    /// creates a ForwardPath from a string repr. of an mailbox without checking it
    ///
    /// This mothod does not check if the string is grammatically correct
    pub fn mailbox_unchecked<I>(mailbox: I) -> Self
        where I: Into<String>
    {
        ForwardPath(mailbox.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl ReversePath {

    pub fn empty() -> Self {
        ReversePath(None)
    }

    pub fn mailbox_unchecked<I>(mailbox: I) -> Self
        where I: Into<String>
    {
        ReversePath(Some(mailbox.into()))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_ref()
            .map(|os| &**os)
            .unwrap_or("")
    }
}

impl FromStr for EhloParam {
    type Err = SyntaxError;

    fn from_str(inp: &str) -> Result<Self, Self::Err> {
        let valid = inp.bytes().all(|bch| {
            33 <= bch && bch <= 126
        });

        if valid {
            Ok(EhloParam(inp.to_owned().into()))
        } else {
            Err(SyntaxError::Param)
        }
    }
}

impl EsmtpKeyword {

    pub fn new<I>(val: I) -> Result<Self, SyntaxError>
        where I: AsRef<str> + Into<String>
    {
        let valid = {
            let mut iter = val.as_ref().chars();
            iter.next()
                .map(|ch| ch.is_ascii_alphanumeric()).unwrap_or(false)
                && iter.all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        };

        if valid {
            let mut sfyied: String = val.into();
            sfyied.make_ascii_uppercase();
            Ok(EsmtpKeyword(sfyied.into()))
        } else {
            Err(SyntaxError::EsmtpKeyword)
        }
    }
}

impl FromStr for EsmtpKeyword {
    type Err = SyntaxError;

    fn from_str(inp: &str) -> Result<Self, Self::Err> {
        EsmtpKeyword::new(inp)
    }
}

impl EsmtpValue {
    pub fn new<I>(val: I) -> Result<Self, SyntaxError>
        where I: AsRef<str> + Into<String>
    {
        let valid = val.as_ref().bytes().all(|bch| {
            33 <= bch && (bch <= 60 || (62 <= bch && bch <= 128))
        });

        if valid {
            let sfyied: String = val.into();
            Ok(EsmtpValue(sfyied.into()))
        } else {
            Err(SyntaxError::EsmtpKeyword)
        }
    }
}

impl FromStr for EsmtpValue {
    type Err = SyntaxError;

    fn from_str(inp: &str) -> Result<Self, Self::Err> {
        EsmtpValue::new(inp)
    }
}

impl FromStr for Capability {
    type Err = SyntaxError;

    fn from_str(inp: &str) -> Result<Self, Self::Err> {
        EsmtpKeyword::from_str(inp).map(Capability)
    }
}

impl Domain {

    pub fn new_unchecked(domain: String) -> Self {
        Domain(domain.into())
    }
}

impl FromStr for Domain {
    type Err = SyntaxError;

    fn from_str(inp: &str) -> Result<Self, Self::Err> {
        let valid = inp.split(".").all(validate_subdomain);

        if valid {
            Ok(Domain(inp.to_lowercase().into()))
        } else {
            Err(SyntaxError::Domain)
        }
    }
}

fn validate_subdomain(inp: &str) -> bool {
    let len = inp.len();
    let binp = inp.as_bytes();
    len > 1
        && binp[0].is_ascii_alphanumeric()
        && binp[1..len-1].iter().all(|bch| bch.is_ascii_alphanumeric() || *bch == b'-' )
        && binp[len - 1].is_ascii_alphanumeric()
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum SyntaxError {
    Domain,
    Param,
    AddressLiteral,
    EsmtpValue,
    EsmtpKeyword,
}

impl Display for SyntaxError {

    fn fmt(&self, fter: &mut fmt::Formatter) -> fmt::Result {
        write!(fter, "{}", self.description())
    }
}

impl Error for SyntaxError {
    fn description(&self) -> &str {
        use self::SyntaxError::*;
        match *self {
            Domain => "syntax error parsing Domain from str",
            Param => "syntax error parsing Param str",
            EsmtpKeyword => "syntax error parsing esmtp-keyword from str",
            EsmtpValue => "syntax error parsing esmtp-value from str",
            AddressLiteral => "syntax error parsing address-literal from str",
        }
    }
}

impl AddressLiteral {

    /// Create a "general" AddressLiteral which is not IPv4/v6
    ///
    /// This is mainly for enabling other alternatives to IPv4/IPv6.
    /// Note that RFC 5321 limits it to **standarized** tags, i.e.
    /// tags registered with IANA.
    #[doc(hidden)]
    pub fn custom_literal<AS1, AS2>(standarized_tag: AS1, custom_part: AS2)
        -> Result<Self, SyntaxError>
        where AS1: AsRef<str>, AS2: AsRef<str>
    {
        let tag = standarized_tag.as_ref();
        let valid_tag = tag.as_bytes()
            .last().map(|bch| *bch != b'-').unwrap_or(false)
            && tag.bytes().all(|bch| bch.is_ascii_alphanumeric() || bch == b'-');

        if !valid_tag {
            return Err(SyntaxError::AddressLiteral);
        }

        let custom_part = custom_part.as_ref();
        let valid = custom_part.bytes().all(|bch| {
            (33 <= bch && bch <= 90) || (94 <= bch && bch <= 126)
        });

        if valid {
            Ok(AddressLiteral(format!("[{}:{}]", tag, custom_part).into()))
        } else {
            Err(SyntaxError::AddressLiteral)
        }
    }
}


impl From<IpAddr> for AddressLiteral {
    fn from(addr: IpAddr) -> Self {
        use self::IpAddr::*;
        match addr {
            V4(ref addr) => AddressLiteral::from(addr),
            V6(ref addr) => AddressLiteral::from(addr)
        }
    }
}

impl From<Ipv4Addr> for AddressLiteral {
    fn from(addr: Ipv4Addr) -> Self {
        AddressLiteral::from(&addr)
    }
}

impl From<Ipv6Addr> for AddressLiteral {
    fn from(addr: Ipv6Addr) -> Self {
        AddressLiteral::from(&addr)
    }
}

impl<'a> From<&'a Ipv4Addr> for AddressLiteral {
    fn from(addr: &'a Ipv4Addr) -> Self {
        AddressLiteral(format!("[{}]", addr).into())
    }
}

impl<'a> From<&'a Ipv6Addr> for AddressLiteral {
    fn from(addr: &'a Ipv6Addr) -> Self {
        AddressLiteral(format!("[IPv6:{}]", addr).into())
    }
}

#[cfg(test)]
mod test {
    #![allow(non_snake_case)]

    mod EhloParams {
        use super::super::EhloParam;

        #[test]
        fn case_sensitive() {
            let a: EhloParam = "affen".parse().unwrap();
            let b: EhloParam = "AFFEN".parse().unwrap();
            assert_ne!(a, b);
            assert_ne!(a, "aFFen")
        }

        #[test]
        fn displayed_unchanged() {
            let a: EhloParam = "afFen".parse().unwrap();
            let s: String = a.into();
            assert_eq!(s, "afFen")
        }
    }

    mod EsmtpKeyword {
        use super::super::EsmtpKeyword;

        #[test]
        fn case_insensitive() {
            let a: EsmtpKeyword = "affen".parse().unwrap();
            let b: EsmtpKeyword = "AFFEN".parse().unwrap();
            let c: EsmtpKeyword = "AffEN".parse().unwrap();
            assert_eq!(a, b);
            assert_eq!(b, c);
            assert_eq!(a, "aFFen");
        }

        #[test]
        fn displayed_uppercase() {
            let a: EsmtpKeyword = "afFen".parse().unwrap();
            let s: String = a.into();
            assert_eq!(s, "AFFEN")
        }
    }

    mod EsmtpValue {
        use super::super::EsmtpValue;

        #[test]
        fn case_sensitive() {
            let a: EsmtpValue = "affen".parse().unwrap();
            let b: EsmtpValue = "AFFEN".parse().unwrap();
            assert_ne!(a, b);
            assert_ne!(a, "aFFen");
        }

        #[test]
        fn displayed_unchanged() {
            let a: EsmtpValue = "afFen".parse().unwrap();
            let s: String = a.into();
            assert_eq!(s, "afFen")
        }

    }

    mod Capability {
        use std::collections::HashMap;
        use ::ascii::IgnoreAsciiCaseStr;
        use super::super::Capability;

        #[test]
        fn has_to_work_with_hashmaps() {
            let mut map = HashMap::new();
            let cap: Capability = "smtputf8".parse().unwrap();
            let cap2: Capability = "SmtpUtf8".parse().unwrap();
            map.insert(cap, ());

            assert!(map.contains_key(&cap2))
        }

        #[test]
        fn has_to_work_with_hashmaps_and_str() {
            let mut map = HashMap::new();
            let cap: Capability = "smtputf8".parse().unwrap();
            map.insert(cap, ());

            let str_key = "smtPUTf8";
            let wrapped = <&IgnoreAsciiCaseStr>::from(str_key);
            assert!(map.contains_key(wrapped))
        }
    }

    mod Domain {
        use super::super::Domain;

        #[test]
        fn case_insensitive() {
            let a: Domain = "affen".parse().unwrap();
            let b: Domain = "AFFEN".parse().unwrap();
            let c: Domain = "AffEN".parse().unwrap();
            assert_eq!(a, b);
            assert_eq!(b, c);
            assert_eq!(a, "aFFen");
        }

        #[test]
        fn displayed_lowercase() {
            let a: Domain = "afFen".parse().unwrap();
            let s: String = a.into();
            assert_eq!(s, "affen")
        }
    }
}
