# The JNI entry points are resolved by name at runtime, so R8 must not rename
# or strip NativeCore or the callback interface the Rust side invokes.
-keepclasseswithmembernames class com.doubleslash.client.NativeCore {
    native <methods>;
}
-keep class com.doubleslash.client.NativeCore { *; }
-keep interface com.doubleslash.client.NativeCore$EventSink { *; }

# kotlinx.serialization generates serializers reflectively from these.
-keepattributes *Annotation*, InnerClasses
-dontnote kotlinx.serialization.**
-keepclassmembers class com.doubleslash.client.** {
    *** Companion;
}
-keepclasseswithmembers class com.doubleslash.client.** {
    kotlinx.serialization.KSerializer serializer(...);
}
