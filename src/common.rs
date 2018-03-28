use std::collections::HashMap;
use std::convert::AsRef;
use std::borrow::Borrow;
use std::str::FromStr;
use std::fmt::{self, Display};
use std::error::Error;
use std::net::{SocketAddr, SocketAddrV4, SocketAddrV6};

use ascii::{IgnoreAsciiCaseStr, IgnoreAsciiCaseString};

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
pub struct Capability(IgnoreAsciiCaseString);

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct EhloParam(String);

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Domain(IgnoreAsciiCaseString);

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct AddressLiteral(String);

macro_rules! impl_str_wrapper {
    ($($name:ident),*) => ($(

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
                &self.0 == other
            }
        }

        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                &self.0 == other
            }
        }

    )*);
}

macro_rules! impl_str_no_case_wrapper {
    ($($name:ident),*) => ($(
        impl Borrow<IgnoreAsciiCaseStr> for $name {
            fn borrow(&self) -> &IgnoreAsciiCaseStr {
                self.0.as_ref()
            }
        }
    )*);
}

macro_rules! impl_str_case_wrapper {
    ($($name:ident),*) => ($(
        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.0.as_ref()
            }
        }
    )*);
}

impl_str_wrapper!(EhloParam, Capability, Domain, AddressLiteral);
impl_str_no_case_wrapper!(Capability, Domain);
impl_str_case_wrapper!(EhloParam);


impl FromStr for EhloParam {
    type Err = EhloSyntaxError;

    fn from_str(inp: &str) -> Result<Self, Self::Err> {
        let valid = inp.chars().all(|ch| {
            let cp = ch as u32;
            33 <= cp && cp <= 126
        });

        if valid {
            Ok(EhloParam(inp.to_owned()))
        } else {
            Err(EhloSyntaxError::Param)
        }
    }
}


impl FromStr for Capability {
    type Err = EhloSyntaxError;

    fn from_str(inp: &str) -> Result<Self, Self::Err> {
        let mut iter = inp.chars();

        let valid = iter.next()
            .map(|ch| ch.is_ascii_alphanumeric()).unwrap_or(false)
            && iter.all(|ch| ch.is_ascii_alphanumeric() || ch == '-');

        if valid {
            Ok(Capability(inp.to_uppercase().into()))
        } else {
            Err(EhloSyntaxError::Capability)
        }
    }
}

impl FromStr for Domain {
    type Err = EhloSyntaxError;

    fn from_str(inp: &str) -> Result<Self, Self::Err> {
        let valid = inp.split(".").all(validate_subdomain);

        if valid {
            Ok(Domain(inp.to_lowercase().into()))
        } else {
            Err(EhloSyntaxError::Domain)
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
pub enum EhloSyntaxError {
    Domain,
    Param,
    Capability,
    AddressLiteral
}

impl Display for EhloSyntaxError {

    fn fmt(&self, fter: &mut fmt::Formatter) -> fmt::Result {
        write!(fter, "{}", self.description())
    }
}

impl Error for EhloSyntaxError {
    fn description(&self) -> &str {
        use self::EhloSyntaxError::*;
        match *self {
            Domain => "syntax error parsing Domain from str",
            Param => "syntax error parsing Param str",
            Capability => "syntax error parsing Capability from str",
            AddressLiteral => "syntax error parsing AddressLiteral from str",
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
        -> Result<Self, EhloSyntaxError>
        where AS1: AsRef<str>, AS2: AsRef<str>
    {
        let tag = standarized_tag.as_ref();
        let valid_tag = tag.as_bytes()
            .last().map(|bch| *bch != b'-').unwrap_or(false)
            && tag.bytes().all(|bch| bch.is_ascii_alphanumeric() || bch == b'-');

        if !valid_tag {
            return Err(EhloSyntaxError::AddressLiteral);
        }

        let custom_part = custom_part.as_ref();
        let valid = custom_part.bytes().all(|bch| {
            (33 <= bch && bch <= 90) || (94 <= bch && bch <= 126)
        });

        if valid {
            Ok(AddressLiteral(format!("{}:{}", tag, custom_part)))
        } else {
            Err(EhloSyntaxError::AddressLiteral)
        }
    }
}


impl From<SocketAddr> for AddressLiteral {
    fn from(addr: SocketAddr) -> Self {
        use self::SocketAddr::*;
        match addr {
            V4(ref addr) => AddressLiteral::from(addr),
            V6(ref addr) => AddressLiteral::from(addr)
        }
    }
}

impl From<SocketAddrV4> for AddressLiteral {
    fn from(addr: SocketAddrV4) -> Self {
        AddressLiteral::from(&addr)
    }
}

impl From<SocketAddrV6> for AddressLiteral {
    fn from(addr: SocketAddrV6) -> Self {
        AddressLiteral::from(&addr)
    }
}

impl<'a> From<&'a SocketAddrV4> for AddressLiteral {
    fn from(addr: &'a SocketAddrV4) -> Self {
        AddressLiteral(format!("{}", addr))
    }
}

impl<'a> From<&'a SocketAddrV6> for AddressLiteral {
    fn from(addr: &'a SocketAddrV6) -> Self {
        AddressLiteral(format!("IPv6:{}", addr))
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

    mod Capabilities {
        use std::collections::HashMap;
        use ::ascii::IgnoreAsciiCaseStr;
        use super::super::Capability;

        #[test]
        fn case_insensitive() {
            let a: Capability = "affen".parse().unwrap();
            let b: Capability = "AFFEN".parse().unwrap();
            let c: Capability = "AffEN".parse().unwrap();
            assert_eq!(a, b);
            assert_eq!(b, c);
            assert_eq!(a, "aFFen");
        }

        #[test]
        fn displayed_uppercase() {
            let a: Capability = "afFen".parse().unwrap();
            let s: String = a.into();
            assert_eq!(s, "AFFEN")
        }

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
