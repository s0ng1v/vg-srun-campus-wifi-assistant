pub mod crypto;
pub mod discovery;
pub mod error;
pub mod model;
pub mod portal;

pub use crypto::{build_username, hmac_md5_hex, sha1_hex, srun_base64_encode, xencode};
pub use discovery::{discover_portal, enrich_portal_profile, portal_profile_from_login_url};
pub use error::{LoginFailure, LoginFailureKind, SrunError};
pub use model::{PortalProfile, SrunAccount, UserInfo};
pub use portal::{probe_external, ExternalProbe, PortalClient};
