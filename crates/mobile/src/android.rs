//! What the Android app hands the core once, from the JVM, before anything
//! goes over HTTPS: its `Context`, through which rustls-platform-verifier
//! checks certificates with Android's own trust store (the sync's WebDAV and
//! S3). Without it the first HTTPS request would panic in the verifier.

use jni::EnvUnowned;
use jni::objects::{JClass, JObject};

/// `com.hivepaas.shellwing.Native.init(context)`, called by the app before
/// it makes its `Shellwing`. A second call is a no-op.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hivepaas_shellwing_Native_init<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    context: JObject<'caller>,
) {
    env.with_env(|env| rustls_platform_verifier::android::init_with_env(env, context))
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
