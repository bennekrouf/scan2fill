# Rust finds this class and its methods by name, through JNI: the release
# build's minifier sees no caller and would remove or rename them.
-keep class com.mayorana.scan2fil.platform.Scan2filPlatform { *; }
