use crate::{error::MissingCapabilities, Capability, EhloData, EsmtpKeyword};

mod login;
pub use self::login::*;

mod plain;
pub use self::plain::*;

const CAP_AUTH: &str = "AUTH";

fn validate_auth_capability(
    caps: Option<&EhloData>,
    auth_kind: &'static str,
) -> Result<(), MissingCapabilities> {
    caps.and_then(|ehlo_data| ehlo_data.get_capability_params(CAP_AUTH))
        .and_then(|auth_methos| {
            auth_methos
                .iter()
                .find(|method| method.as_str().eq_ignore_ascii_case(auth_kind))
        })
        .map(|_| ())
        .ok_or_else(|| {
            //FIXME specify it to be auth login
            let mcap = Capability::from(EsmtpKeyword::from_unchecked(CAP_AUTH));
            MissingCapabilities::new(vec![mcap])
        })
}
