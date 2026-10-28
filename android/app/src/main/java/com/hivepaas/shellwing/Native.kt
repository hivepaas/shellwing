package com.hivepaas.shellwing

import android.content.Context

/** The Rust library's own JNI entry (crates/mobile/src/android.rs). */
object Native {
    init {
        System.loadLibrary("shellwing_mobile")
    }

    /**
     * Hands the core the app's context, through which certificates are
     * checked with Android's trust store. Before the first `Shellwing`.
     */
    @JvmStatic
    external fun init(context: Context)
}
