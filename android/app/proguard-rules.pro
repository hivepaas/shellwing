# JNA and the UniFFI bindings find their classes and fields by name.
-keep class com.sun.jna.** { *; }
-keep class * extends com.sun.jna.** { *; }
-keep class com.hivepaas.shellwing.core.** { *; }
-dontwarn java.awt.**
# The Rust side calls rustls-platform-verifier's Kotlin half through JNI.
-keep, includedescriptorclasses class org.rustls.platformverifier.** { *; }
# Called from Rust by its JNI name.
-keep class com.hivepaas.shellwing.Native { *; }
