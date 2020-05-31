use std::borrow::{Borrow, ToOwned};
use std::hash::{Hash, Hasher};
use std::ops::Deref;

/// A string which ignores ascii case when compared
#[derive(Debug, Eq, Clone)]
pub struct IgnoreAsciiCaseString {
    inner: String,
}

impl IgnoreAsciiCaseString {
    pub fn as_str(&self) -> &str {
        &self.inner
    }
}

impl AsRef<str> for IgnoreAsciiCaseString {
    fn as_ref(&self) -> &str {
        &self.inner
    }
}

impl AsRef<IgnoreAsciiCaseStr> for IgnoreAsciiCaseString {
    fn as_ref(&self) -> &IgnoreAsciiCaseStr {
        self.borrow()
    }
}

impl Borrow<IgnoreAsciiCaseStr> for IgnoreAsciiCaseString {
    fn borrow(&self) -> &IgnoreAsciiCaseStr {
        let as_str = &*self.inner;
        as_str.into()
    }
}

impl<'a> From<&'a str> for IgnoreAsciiCaseString {
    fn from(v: &'a str) -> IgnoreAsciiCaseString {
        v.to_owned().into()
    }
}

impl From<String> for IgnoreAsciiCaseString {
    fn from(v: String) -> Self {
        IgnoreAsciiCaseString { inner: v }
    }
}

impl Into<String> for IgnoreAsciiCaseString {
    fn into(self) -> String {
        let IgnoreAsciiCaseString { inner } = self;
        inner
    }
}

impl Hash for IgnoreAsciiCaseString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        ignore_ascii_hash(&*self.inner, state)
    }
}

impl Deref for IgnoreAsciiCaseString {
    type Target = IgnoreAsciiCaseStr;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

/// a `str` which uses ascii case when compared
#[derive(Debug, Eq)]
#[repr(C)]
pub struct IgnoreAsciiCaseStr {
    inner: str,
}

impl IgnoreAsciiCaseStr {
    pub fn as_str(&self) -> &str {
        self.into()
    }
}

impl ToOwned for IgnoreAsciiCaseStr {
    type Owned = IgnoreAsciiCaseString;

    fn to_owned(&self) -> Self::Owned {
        let as_str: &str = self.into();
        let as_string = as_str.to_owned();
        as_string.into()
    }
}

impl AsRef<str> for IgnoreAsciiCaseStr {
    fn as_ref(&self) -> &str {
        self.into()
    }
}

impl<'a> Into<&'a str> for &'a IgnoreAsciiCaseStr {
    fn into(self) -> &'a str {
        &self.inner
    }
}

impl<'a> Into<String> for &'a IgnoreAsciiCaseStr {
    fn into(self) -> String {
        self.inner.to_owned()
    }
}

impl<'a> From<&'a str> for &'a IgnoreAsciiCaseStr {
    fn from(v: &'a str) -> &'a IgnoreAsciiCaseStr {
        let v = v as *const str as *const IgnoreAsciiCaseStr;
        unsafe { &*v }
    }
}

impl Hash for IgnoreAsciiCaseStr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        ignore_ascii_hash(&self.inner, state)
    }
}

macro_rules! impl_eq {
    ($($name:ident),*) => ($(
        impl PartialEq<IgnoreAsciiCaseString> for $name {
            fn eq(&self, other: &IgnoreAsciiCaseString) -> bool {
                self.as_str().eq_ignore_ascii_case(other.as_str())
            }
        }

        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                self.as_str().eq_ignore_ascii_case(other.as_str())
            }
        }

        impl PartialEq<IgnoreAsciiCaseStr> for $name {
            fn eq(&self, other: &IgnoreAsciiCaseStr) -> bool {
                self.as_str().eq_ignore_ascii_case(other.as_str())
            }
        }

        impl<'a> PartialEq<&'a IgnoreAsciiCaseStr> for $name {
            fn eq(&self, other: &&'a IgnoreAsciiCaseStr) -> bool {
                self.as_str().eq_ignore_ascii_case(other.as_str())
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.as_str().eq_ignore_ascii_case(other)
            }
        }

        impl<'a> PartialEq<&'a str> for $name {
            fn eq(&self, other: &&'a str) -> bool {
                self.as_str().eq_ignore_ascii_case(*other)
            }
        }
    )*);
}

impl_eq!(IgnoreAsciiCaseString, IgnoreAsciiCaseStr);

fn ignore_ascii_hash<H>(data: &str, state: &mut H)
where
    H: Hasher,
{
    for bch in data.bytes() {
        let lb = bch.to_ascii_lowercase();
        state.write_u8(lb)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn can_be_used_with_hash_map() {
        let mut map = HashMap::new();
        let e1 = IgnoreAsciiCaseString::from("test");
        let str_e1 = <&IgnoreAsciiCaseStr>::from("test");
        let str_e1v2 = <&IgnoreAsciiCaseStr>::from("tESt");

        map.insert(e1.clone(), ());
        assert!(map.contains_key(&e1));
        assert!(map.contains_key(str_e1));
        assert!(map.contains_key(str_e1v2));
    }
}
