// ignore_for_file: constant_identifier_names

part of '../runtime.dart';

class PushArg implements EvcOp {
  PushArg(Runtime runtime) : _location = runtime._readInt16();

  PushArg.make(this._location);

  final int _location;

  static const int LEN = Evc.BASE_OPLEN + Evc.I16_LEN;

  // Set value at position to constant
  @override
  void run(Runtime runtime) {
    final value = runtime.frame[_location];
    if (runtime.debugTraceArgs) {
      print('[dart_eval][PushArg] L$_location -> ${value?.runtimeType}: $value');
    }
    runtime.args.add(value);
  }

  @override
  String toString() => 'PushArg (L$_location)';
}

class Pop implements EvcOp {
  Pop(Runtime runtime) : _amount = runtime._readUint8();

  Pop.make(this._amount);

  final int _amount;

  static const int LEN = Evc.BASE_OPLEN;

  @override
  void run(Runtime runtime) {
    if (runtime.debugTraceArgs) {
      final before = runtime.frameOffset;
      final after = before - _amount;
      print('[dart_eval][Pop] -$_amount frameOffset $before -> $after');
    }
    runtime.frameOffset -= _amount;
  }

  @override
  String toString() => 'Pop ($_amount)';
}

class PushReturnValue implements EvcOp {
  PushReturnValue(Runtime runtime);

  PushReturnValue.make();

  static const int LEN = Evc.BASE_OPLEN;

  @override
  void run(Runtime runtime) {
    final offset = runtime.frameOffset++;
    if (runtime.returnValue is $Future || runtime.returnValue is Future) {
      runtime.lastFutureValue = runtime.returnValue;
    }
    if (runtime.debugTraceArgs) {
      final rv = runtime.returnValue;
      print('[dart_eval][PushReturnValue] L$offset <- ${rv?.runtimeType}: $rv');
    }
    runtime.frame[offset] = runtime.returnValue;
  }

  @override
  String toString() => 'PushReturnValue ()';
}

class SetReturnValue implements EvcOp {
  SetReturnValue(Runtime runtime) : _location = runtime._readInt16();

  SetReturnValue.make(this._location);

  final int _location;

  static const int LEN = Evc.BASE_OPLEN + Evc.I16_LEN;

  @override
  void run(Runtime runtime) {
    runtime.returnValue = runtime.frame[_location] as int;
  }

  @override
  String toString() => 'SetReturnValue (\$ = L$_location)';
}

class CopyValue implements EvcOp {
  CopyValue(Runtime exec)
      : _to = exec._readInt16(),
        _from = exec._readInt16();

  CopyValue.make(this._to, this._from);

  final int _to;
  final int _from;

  static const LEN = Evc.BASE_OPLEN + Evc.I16_LEN * 2;

  // Conditional move
  @override
  void run(Runtime runtime) {
    final from = runtime.frame[_from];
    if (runtime.debugTraceArgs) {
      print('[dart_eval][CopyValue] L$_to <- L$_from (${from?.runtimeType}: $from)');
    }
    runtime.frame[_to] = from;
  }

  @override
  String toString() => 'CopyValue (L$_to <-- L$_from)';
}

class LoadGlobal implements EvcOp {
  LoadGlobal(Runtime runtime) : _index = runtime._readInt32();

  final int _index;

  LoadGlobal.make(this._index);

  static const LEN = Evc.BASE_OPLEN + Evc.I32_LEN + Evc.I16_LEN;

  @override
  void run(Runtime runtime) {
    var value = runtime.globals[_index];
    if (value == null) {
      runtime.callStack.add(runtime._prOffset);
      runtime.catchStack.add([]);
      runtime._prOffset = runtime.globalInitializers[_index];
    } else {
      runtime.returnValue = value;
    }
  }

  @override
  String toString() => 'LoadGlobal (G$_index)';
}

class SetGlobal implements EvcOp {
  SetGlobal(Runtime runtime)
      : _index = runtime._readInt32(),
        _value = runtime._readInt16();

  final int _index;
  final int _value;

  SetGlobal.make(this._index, this._value);

  static const LEN = Evc.BASE_OPLEN + Evc.I32_LEN + Evc.I16_LEN;

  @override
  void run(Runtime runtime) {
    final value = runtime.frame[_value];
    runtime.globals[_index] = value;
  }

  @override
  String toString() => 'SetGlobal (G$_index = L$_value)';
}
