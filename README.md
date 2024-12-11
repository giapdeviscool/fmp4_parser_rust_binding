```sh
mkdir -p jniLibs/arm64-v8a/ && \
  cp target/aarch64-linux-android/debug/libreverse.so jniLibs/arm64-v8a/libuniffi_reverse.so && \
  mkdir -p jniLibs/armeabi-v7a/ && \
    cp target/armv7-linux-androideabi/debug/libreverse.so jniLibs/armeabi-v7a/libuniffi_reverse.so && \
  mkdir -p jniLibs/x86/ && \
    cp target/i686-linux-android/debug/libreverse.so jniLibs/x86/libuniffi_reverse.so && \
  mkdir -p jniLibs/x86_64/ && \
    cp target/x86_64-linux-android/debug/libreverse.so jniLibs/x86_64/libuniffi_reverse.so
```