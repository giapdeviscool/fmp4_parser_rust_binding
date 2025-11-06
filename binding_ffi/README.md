1. Build binary lib

- set up target
   rustup target add {{target}}
- ios

```sh
#/binding_ffi
# Mac os
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
# iOS
cargo build --release --target aarch64-apple-ios
# iOS Simulator
cargo build --release --target aarch64-apple-ios-sim
cargo build --release --target x86_64-apple-ios
```

- android

* cross

```sh
#/binding_ffi
cross build --target x86_64-linux-android && \
    cross build --target i686-linux-android && \
    cross build --target armv7-linux-androideabi && \
    cross build --target aarch64-linux-android
```

2. back to main folder

```text
cd ../
```

# 

3. generate android interface file

```sh
   
   cargo run --bin uniffi-bindgen generate --library target/x86_64-linux-android/release/libbinding_ffi.so --language kotlin --out-dir out
```

then check out the out folder

```sh

```

```sh
#/binding_adroid_box
mkdir -p jniLibs1/arm64-v8a/ && \
  cp target/aarch64-linux-android/release/libbinding_ffi.so jniLibs1/arm64-v8a/libbinding_ffi.so && \
  mkdir -p jniLibs1/armeabi-v7a/ && \
    cp target/armv7-linux-androideabi/release/libbinding_ffi.so jniLibs1/armeabi-v7a/libbinding_ffi.so && \
  mkdir -p jniLibs1/x86/ && \
    cp target/i686-linux-android/release/libbinding_ffi.so jniLibs1/x86/libbinding_ffi.so && \
  mkdir -p jniLibs1/x86_64/ && \
    cp target/x86_64-linux-android/release/libbinding_ffi.so jniLibs1/x86_64/libbinding_ffi.so
```

4. generate swift interface file

```sh
cargo run --bin uniffi-bindgen generate --library target/aarch64-apple-ios/debug/libermis_ffi.dylib --language swift --out-dir out/ios
```

```sh
mkdir -p bindingLibs/aarch64-apple-ios/ && \
  cp target/aarch64-apple-ios/release/libbinding_ffi.a bindingLibs/aarch64-apple-ios/libbinding_ffi.a && \
  mkdir -p bindingLibs/aarch64-apple-ios-sim/ && \
    cp target/aarch64-apple-ios-sim/release/libbinding_ffi.a bindingLibs/aarch64-apple-ios-sim/libbinding_ffi.a && \
  mkdir -p bindingLibs/x86_64-apple-ios/ && \
    cp target/x86_64-apple-ios/release/libbinding_ffi.a bindingLibs/x86_64-apple-ios/libbinding_ffi.a
```

- test

1. create client with wallet and db_path
   client = createClient(db_path,null,accountAddress, name)
2. export client_key_package
   keyPackage vec<u8> (bytes) = client.keyPackage()
3. create group
   group1 {group_id,proposal, commit, welcome} = client.createGroupWithGroupIdAndMembers(group_id: string, vec<keyPackage>)
4. check group
   group_id : vec<u8> bytes = group.group.checkGroupId() -> decode to string
5. export group ratchet tree
   ratchettree : vec<u8> (bytes) = group1.group.exportRatchetTree()
6. join from invite
   group2 = client.joinGroupByWelcome(group1.welcome, ratchettree)
7. check group2 id
   group2_id : vec<u8> bytes = group.group.checkGroupId() -> decode to string
8. load_group:
   group: group_id: vec<u8>
