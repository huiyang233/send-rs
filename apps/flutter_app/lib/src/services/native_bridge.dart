import 'dart:convert';
import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';

class NativeBridge {
  NativeBridge._(this._lib);

  final DynamicLibrary _lib;

  late final int Function() _startDiscovery =
      _lib.lookupFunction<Int32 Function(), int Function()>('start_discovery');

  late final Pointer<Utf8> Function() _listPeers =
      _lib.lookupFunction<Pointer<Utf8> Function(), Pointer<Utf8> Function()>('list_peers');

  late final void Function(Pointer<Utf8>) _freeCString =
      _lib.lookupFunction<Void Function(Pointer<Utf8>), void Function(Pointer<Utf8>)>('free_c_string');

  static NativeBridge open() {
    if (Platform.isMacOS) {
      return NativeBridge._(DynamicLibrary.open('libsendrs_ffi.dylib'));
    }
    if (Platform.isWindows) {
      return NativeBridge._(DynamicLibrary.open('sendrs_ffi.dll'));
    }
    if (Platform.isAndroid) {
      return NativeBridge._(DynamicLibrary.open('libsendrs_ffi.so'));
    }
    throw UnsupportedError('Platform not supported yet');
  }

  int startDiscovery() => _startDiscovery();

  List<dynamic> listPeers() {
    final ptr = _listPeers();
    if (ptr == nullptr) return const [];
    try {
      final text = ptr.toDartString();
      return jsonDecode(text) as List<dynamic>;
    } finally {
      _freeCString(ptr);
    }
  }
}
