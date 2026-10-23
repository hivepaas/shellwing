//! The system's own way to know the user is there, holding a secret for
//! shellwing's vault that it gives only then:
//!
//! - **Windows Hello** (a fingerprint, the face, or the device's PIN): a key
//!   in the TPM, made for shellwing, that signs only after Hello. It signs
//!   one challenge, always the same; the secret is the signature's SHA-256
//!   (RSA PKCS#1 v1.5 signatures do not change). KeePass's Windows Hello
//!   plugins do the same.
//! - **Touch ID** (macOS): the secret is a Keychain item that opens only to
//!   Touch ID with the fingers enrolled now (a finger added makes it gone,
//!   and the passphrase is asked for). The data protection keychain takes
//!   such items only from apps signed with a Developer ID: an unsigned build
//!   is told so.
//!
//! Elsewhere there is none. Calls block on the system's prompt: never on
//! the UI thread.

pub mod confirm;

/// Windows Hello or Touch ID, where this system has it.
pub struct System(Kind);

enum Kind {
    #[cfg(windows)]
    Hello,
    #[cfg(target_os = "macos")]
    TouchId,
}

/// What a lookup that the user did not allow says.
pub enum Secret {
    Given([u8; 32]),
    /// They said no, or it is gone (Touch ID's fingers changed, Hello reset).
    Refused,
}

impl System {
    /// The way this system has, if it has one set up. Windows asks Hello
    /// whether it is; a Mac is taken to have Touch ID, and says otherwise
    /// when asked.
    pub fn here() -> Option<System> {
        #[cfg(windows)]
        {
            // Asked once: the answer does not change while running.
            static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            if *SUPPORTED.get_or_init(hello::supported) {
                return Some(System(Kind::Hello));
            }
        }
        #[cfg(target_os = "macos")]
        {
            return Some(System(Kind::TouchId));
        }
        #[allow(unreachable_code)]
        None
    }

    pub fn name(&self) -> &'static str {
        match self.0 {
            #[cfg(windows)]
            Kind::Hello => "Windows Hello",
            #[cfg(target_os = "macos")]
            Kind::TouchId => "Touch ID",
        }
    }

    /// Makes the secret for `id` (replacing one there was) and gives it; the
    /// system asks the user, saying `reason` where it can.
    pub fn enroll(&self, id: &str, reason: &str) -> Result<[u8; 32], String> {
        let _ = (id, reason);
        match self.0 {
            #[cfg(windows)]
            Kind::Hello => hello::enroll(id),
            #[cfg(target_os = "macos")]
            Kind::TouchId => touch_id::enroll(id),
        }
    }

    /// The secret for `id`; the system asks the user.
    pub fn secret(&self, id: &str, reason: &str) -> Result<Secret, String> {
        let _ = (id, reason);
        match self.0 {
            #[cfg(windows)]
            Kind::Hello => hello::secret(id),
            #[cfg(target_os = "macos")]
            Kind::TouchId => touch_id::secret(id),
        }
    }

    /// Forgets the secret for `id`.
    pub fn forget(&self, id: &str) -> Result<(), String> {
        let _ = id;
        match self.0 {
            #[cfg(windows)]
            Kind::Hello => hello::forget(id),
            #[cfg(target_os = "macos")]
            Kind::TouchId => touch_id::forget(id),
        }
    }
}

/// The name the system keeps it under, per vault.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
fn name_for(id: &str) -> String {
    format!("shellwing vault {id}")
}

#[cfg(windows)]
mod hello {
    use sha2::{Digest, Sha256};
    use windows::Security::Credentials::{
        KeyCredential, KeyCredentialCreationOption, KeyCredentialManager, KeyCredentialStatus,
    };
    use windows::Security::Cryptography::CryptographicBuffer;

    use super::{Secret, name_for};

    /// What the key signs: always the same, so the secret is too.
    const CHALLENGE: &[u8] = b"shellwing: the secret that opens the vault, signed by Windows Hello";

    pub fn supported() -> bool {
        KeyCredentialManager::IsSupportedAsync()
            .and_then(|op| op.join())
            .unwrap_or(false)
    }

    fn err(e: windows::core::Error) -> String {
        e.message().to_string()
    }

    fn status(s: KeyCredentialStatus) -> String {
        match s {
            KeyCredentialStatus::UserCanceled => "cancelled".into(),
            KeyCredentialStatus::NotFound => "Windows Hello has no key for shellwing".into(),
            KeyCredentialStatus::SecurityDeviceLocked => {
                "the security device is locked: too many tries".into()
            }
            KeyCredentialStatus::UserPrefersPassword => "the passphrase was chosen".into(),
            _ => "Windows Hello did not sign".into(),
        }
    }

    fn sign(key: &KeyCredential) -> Result<Option<[u8; 32]>, String> {
        let challenge = CryptographicBuffer::CreateFromByteArray(CHALLENGE).map_err(err)?;
        let signed = key
            .RequestSignAsync(&challenge)
            .and_then(|op| op.join())
            .map_err(err)?;
        match signed.Status().map_err(err)? {
            KeyCredentialStatus::Success => {}
            KeyCredentialStatus::UserCanceled | KeyCredentialStatus::UserPrefersPassword => {
                return Ok(None);
            }
            s => return Err(status(s)),
        }
        let mut bytes = windows::core::Array::<u8>::new();
        CryptographicBuffer::CopyToByteArray(&signed.Result().map_err(err)?, &mut bytes)
            .map_err(err)?;
        Ok(Some(Sha256::digest(&bytes[..]).into()))
    }

    pub fn enroll(id: &str) -> Result<[u8; 32], String> {
        let made = KeyCredentialManager::RequestCreateAsync(
            &name_for(id).into(),
            KeyCredentialCreationOption::ReplaceExisting,
        )
        .and_then(|op| op.join())
        .map_err(err)?;
        match made.Status().map_err(err)? {
            KeyCredentialStatus::Success => {}
            s => return Err(status(s)),
        }
        let key = made.Credential().map_err(err)?;
        sign(&key)?.ok_or_else(|| "cancelled".to_string())
    }

    pub fn secret(id: &str) -> Result<Secret, String> {
        let opened = KeyCredentialManager::OpenAsync(&name_for(id).into())
            .and_then(|op| op.join())
            .map_err(err)?;
        match opened.Status().map_err(err)? {
            KeyCredentialStatus::Success => {}
            KeyCredentialStatus::NotFound => return Ok(Secret::Refused),
            s => return Err(status(s)),
        }
        let key = opened.Credential().map_err(err)?;
        Ok(match sign(&key)? {
            Some(s) => Secret::Given(s),
            None => Secret::Refused,
        })
    }

    pub fn forget(id: &str) -> Result<(), String> {
        match KeyCredentialManager::DeleteAsync(&name_for(id).into()).and_then(|op| op.join()) {
            Ok(()) => Ok(()),
            // Gone already.
            Err(e) if e.code().0 as u32 == 0x8009_0011 => Ok(()),
            Err(e) => Err(err(e)),
        }
    }
}

#[cfg(target_os = "macos")]
mod touch_id {
    use security_framework::access_control::{ProtectionMode, SecAccessControl};
    use security_framework::passwords::{
        delete_generic_password_options, generic_password, set_generic_password_options,
    };
    use security_framework::passwords_options::{AccessControlOptions, PasswordOptions};

    use super::{Secret, name_for};

    const SERVICE: &str = "shellwing vault presence";
    const USER_CANCELED: i32 = -128;
    const AUTH_FAILED: i32 = -25293;
    const NOT_FOUND: i32 = -25300;
    const MISSING_ENTITLEMENT: i32 = -34018;

    fn options(id: &str) -> PasswordOptions {
        let mut o = PasswordOptions::new_generic_password(SERVICE, &name_for(id));
        o.use_protected_keychain();
        o
    }

    fn err(e: security_framework::base::Error) -> String {
        match e.code() {
            MISSING_ENTITLEMENT => {
                "Touch ID needs a signed build of shellwing (this one is not)".into()
            }
            _ => e
                .message()
                .unwrap_or_else(|| format!("the Keychain said {}", e.code())),
        }
    }

    pub fn enroll(id: &str) -> Result<[u8; 32], String> {
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).map_err(|e| e.to_string())?;
        let _ = forget(id);
        let flags = AccessControlOptions::BIOMETRY_CURRENT_SET.bits();
        let access = SecAccessControl::create_with_protection(
            Some(ProtectionMode::AccessibleWhenPasscodeSetThisDeviceOnly),
            flags,
        )
        .map_err(err)?;
        let mut o = options(id);
        o.set_access_control(access);
        o.set_label("shellwing: opens the vault with Touch ID");
        set_generic_password_options(&secret, o).map_err(err)?;
        // Read back at once: the user confirms with Touch ID now, and a
        // Mac without it says so here, not at the next lock.
        match secret_of(id)? {
            Secret::Given(s) if s == secret => Ok(secret),
            Secret::Given(_) => Err("the Keychain gave back another secret".into()),
            Secret::Refused => {
                let _ = forget(id);
                Err("cancelled".into())
            }
        }
    }

    fn secret_of(id: &str) -> Result<Secret, String> {
        match generic_password(options(id)) {
            Ok(bytes) if bytes.len() == 32 => {
                let mut s = [0u8; 32];
                s.copy_from_slice(&bytes);
                Ok(Secret::Given(s))
            }
            Ok(_) => Err("the Keychain item is not shellwing's".into()),
            Err(e) if matches!(e.code(), USER_CANCELED | AUTH_FAILED | NOT_FOUND) => {
                Ok(Secret::Refused)
            }
            Err(e) => Err(err(e)),
        }
    }

    pub fn secret(id: &str) -> Result<Secret, String> {
        secret_of(id)
    }

    pub fn forget(id: &str) -> Result<(), String> {
        match delete_generic_password_options(options(id)) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == NOT_FOUND => Ok(()),
            Err(e) => Err(err(e)),
        }
    }
}
