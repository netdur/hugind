import 'dart:async';
import 'dart:convert';
import 'package:dart_eval/dart_eval.dart';
import 'package:dart_eval/dart_eval_bridge.dart';
import 'package:dart_eval/stdlib/core.dart';
import 'package:dart_eval/src/eval/shared/stdlib/async.dart' as dart_eval_async;
import 'package:dart_eval/src/eval/shared/stdlib/collection.dart'
    as dart_eval_collection;
import 'package:dart_eval/src/eval/shared/stdlib/convert.dart'
    as dart_eval_convert;
import 'package:dart_eval/src/eval/shared/stdlib/core.dart' as dart_eval_core;
import 'package:dart_eval/src/eval/compiler/errors.dart';
import 'capabilities.dart';

String _normalizeRawStrings(String source) {
  bool isIdentChar(int codeUnit) {
    if (codeUnit >= 0x30 && codeUnit <= 0x39) return true; // 0-9
    if (codeUnit >= 0x41 && codeUnit <= 0x5A) return true; // A-Z
    if (codeUnit >= 0x61 && codeUnit <= 0x7A) return true; // a-z
    return codeUnit == 0x5F; // _
  }

  String repeatChar(String ch, int count) {
    var out = '';
    for (var i = 0; i < count; i++) {
      out += ch;
    }
    return out;
  }

  int findDelimiter(String s, int start, String quote, int len) {
    if (len == 1) {
      return s.indexOf(quote, start);
    }
    var i = start;
    while (i <= s.length - len) {
      if (s.substring(i, i + 1) == quote) {
        if (s.substring(i, i + len) == repeatChar(quote, len)) {
          return i;
        }
      }
      i += 1;
    }
    return -1;
  }

  final buf = StringBuffer();
  var i = 0;
  while (i < source.length) {
    final ch = source.substring(i, i + 1);
    if ((ch == 'r' || ch == 'R') && i + 1 < source.length) {
      final next = source.substring(i + 1, i + 2);
      final prevIdent = i > 0 && isIdentChar(source.codeUnitAt(i - 1));
      if (!prevIdent && (next == '\'' || next == '"')) {
        var delimLen = 1;
        if (i + 3 < source.length) {
          final n2 = source.substring(i + 2, i + 3);
          final n3 = source.substring(i + 3, i + 4);
          if (n2 == next && n3 == next) {
            delimLen = 3;
          }
        }

        final startContent = i + 1 + delimLen;
        final end = findDelimiter(source, startContent, next, delimLen);
        if (end >= 0) {
          final rawContent = source.substring(startContent, end);
          final escaped =
              rawContent.replaceAll('\\', '\\\\').replaceAll(r'$', r'\$');
          final delim = repeatChar(next, delimLen);
          buf.write(delim);
          buf.write(escaped);
          buf.write(delim);
          i = end + delimLen;
          continue;
        }
      }
    }

    buf.write(ch);
    i += 1;
  }

  return buf.toString();
}

class AgentSandbox {
  final SysCapability sys;
  final LlmCapability llm;
  final NetworkCapability net;
  final McpCapability mcp;

  AgentSandbox(this.sys, this.llm, this.net, this.mcp);

  Future<void> run(String sourceCode, List<String> args) async {
    final compiler = Compiler();
    final plugins = [
      dart_eval_core.DartCorePlugin(),
      dart_eval_async.DartAsyncPlugin(),
      dart_eval_collection.DartCollectionPlugin(),
      dart_eval_convert.DartConvertPlugin(),
    ];

    for (final plugin in plugins) {
      compiler.addPlugin(plugin);
    }

    compiler.defineBridgeClass(const BridgeClassDef(
        BridgeClassType(
            BridgeTypeRef(BridgeTypeSpec('package:agent/main.dart', 'Bridge')),
            isAbstract: true),
        constructors: {},
        methods: {
          'sysRun': BridgeMethodDef(
              BridgeFunctionDef(
                  returns: BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.future,
                      [BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.string))])),
                  params: [
                    BridgeParameter(
                        'executable',
                        BridgeTypeAnnotation(
                            BridgeTypeRef(CoreTypes.string, [])),
                        false),
                    BridgeParameter(
                        'args',
                        BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.list, [
                          BridgeTypeAnnotation(
                              BridgeTypeRef(CoreTypes.string, []))
                        ])),
                        false),
                    BridgeParameter(
                        'workDir',
                        BridgeTypeAnnotation(
                            BridgeTypeRef(CoreTypes.string, []),
                            nullable: true),
                        false),
                  ],
                  namedParams: []),
              isStatic: true),
          'sysConfirm': BridgeMethodDef(
              BridgeFunctionDef(
                  returns: BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.future,
                      [BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.bool))])),
                  params: [
                    BridgeParameter(
                        'message',
                        BridgeTypeAnnotation(
                            BridgeTypeRef(CoreTypes.string, [])),
                        false)
                  ],
                  namedParams: []),
              isStatic: true),
          'sysReadInput': BridgeMethodDef(
              BridgeFunctionDef(
                  returns:
                      BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.string, [])),
                  params: [
                    BridgeParameter(
                        'prompt',
                        BridgeTypeAnnotation(
                            BridgeTypeRef(CoreTypes.string, [])),
                        false)
                  ],
                  namedParams: []),
              isStatic: true),
          'sysReadFile': BridgeMethodDef(
              BridgeFunctionDef(
                  returns: BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.future,
                      [BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.string))])),
                  params: [
                    BridgeParameter(
                        'path',
                        BridgeTypeAnnotation(
                            BridgeTypeRef(CoreTypes.string, [])),
                        false)
                  ],
                  namedParams: []),
              isStatic: true),
          'sysWriteFile': BridgeMethodDef(
              BridgeFunctionDef(
                  returns: BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.future,
                      [BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.bool))])),
                  params: [
                    BridgeParameter(
                        'path',
                        BridgeTypeAnnotation(
                            BridgeTypeRef(CoreTypes.string, [])),
                        false),
                    BridgeParameter(
                        'contents',
                        BridgeTypeAnnotation(
                            BridgeTypeRef(CoreTypes.string, [])),
                        false)
                  ],
                  namedParams: []),
              isStatic: true),
          'sysExists': BridgeMethodDef(
              BridgeFunctionDef(
                  returns: BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.future,
                      [BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.bool))])),
                  params: [
                    BridgeParameter(
                        'path',
                        BridgeTypeAnnotation(
                            BridgeTypeRef(CoreTypes.string, [])),
                        false)
                  ],
                  namedParams: []),
              isStatic: true),
          'sysMkdir': BridgeMethodDef(
              BridgeFunctionDef(
                  returns: BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.future,
                      [BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.bool))])),
                  params: [
                    BridgeParameter(
                        'path',
                        BridgeTypeAnnotation(
                            BridgeTypeRef(CoreTypes.string, [])),
                        false)
                  ],
                  namedParams: []),
              isStatic: true),
          'sysJsonDecode': BridgeMethodDef(
              BridgeFunctionDef(
                  returns: BridgeTypeAnnotation(
                      BridgeTypeRef(CoreTypes.object, []),
                      nullable: true),
                  params: [
                    BridgeParameter(
                        'source',
                        BridgeTypeAnnotation(
                            BridgeTypeRef(CoreTypes.string, [])),
                        false)
                  ],
                  namedParams: []),
              isStatic: true),
          'sysJsonExtractField': BridgeMethodDef(
              BridgeFunctionDef(
                  returns: BridgeTypeAnnotation(
                      BridgeTypeRef(CoreTypes.object, []),
                      nullable: true),
                  params: [
                    BridgeParameter(
                        'source',
                        BridgeTypeAnnotation(
                            BridgeTypeRef(CoreTypes.string, [])),
                        false),
                    BridgeParameter(
                        'key',
                        BridgeTypeAnnotation(
                            BridgeTypeRef(CoreTypes.string, [])),
                        false)
                  ],
                  namedParams: []),
              isStatic: true),
          'sysPrint': BridgeMethodDef(
              BridgeFunctionDef(
                  returns: BridgeTypeAnnotation(
                      BridgeTypeRef(CoreTypes.voidType, [])),
                  params: [
                    BridgeParameter(
                        'message',
                        BridgeTypeAnnotation(
                            BridgeTypeRef(CoreTypes.string, [])),
                        false)
                  ],
                  namedParams: []),
              isStatic: true),
          'llmChat': BridgeMethodDef(
              BridgeFunctionDef(
                  returns: BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.future,
                      [BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.string))])),
                  params: [
                    BridgeParameter(
                        'prompt',
                        BridgeTypeAnnotation(
                            BridgeTypeRef(CoreTypes.string, [])),
                        false)
                  ],
                  namedParams: []),
              isStatic: true),
          'netFetch': BridgeMethodDef(
              BridgeFunctionDef(
                  returns: BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.future,
                      [BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.string))])),
                  params: [
                    BridgeParameter(
                        'url',
                        BridgeTypeAnnotation(
                            BridgeTypeRef(CoreTypes.string, [])),
                        false)
                  ],
                  namedParams: []),
              isStatic: true),
          'mcpListTools': BridgeMethodDef(
              BridgeFunctionDef(
                  returns: BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.future,
                      [BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.string))])),
                  params: [],
                  namedParams: []),
              isStatic: true),
          'mcpCallTool': BridgeMethodDef(
              BridgeFunctionDef(
                  returns: BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.future,
                      [BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.string))])),
                  params: [
                    BridgeParameter(
                        'name',
                        BridgeTypeAnnotation(
                            BridgeTypeRef(CoreTypes.string, [])),
                        false),
                    BridgeParameter(
                        'args',
                        BridgeTypeAnnotation(BridgeTypeRef(CoreTypes.map, [
                          BridgeTypeAnnotation(
                              BridgeTypeRef(CoreTypes.string, [])),
                          BridgeTypeAnnotation(
                              BridgeTypeRef(CoreTypes.object, []),
                              nullable: true)
                        ])),
                        false)
                  ],
                  namedParams: []),
              isStatic: true),
        },
        bridge: true));

    final prelude = '''
      import 'dart:async';
      import 'dart:convert' as dart_convert;
      
      class Bridge {
         external static Future<String> sysRun(String executable, List<String> args, String? workDir);
         external static Future<bool> sysConfirm(String message);
         external static dynamic sysReadInput(String prompt);
         external static Future<String> sysReadFile(String path);
         external static Future<bool> sysWriteFile(String path, String contents);
         external static Future<bool> sysExists(String path);

         external static Future<bool> sysMkdir(String path);
         external static dynamic sysJsonDecode(String source);
         external static dynamic sysJsonExtractField(String source, String key);
         external static void sysPrint(String message);
         external static Future<String> llmChat(String prompt);
         external static Future<String> netFetch(String url);
         external static Future<String> mcpListTools();
         external static Future<String> mcpCallTool(String name, Map<String, dynamic> args);
      }
      
      dynamic _toPlain(dynamic value) {
        if (value is Map) {
          final out = <String, dynamic>{};
          for (final key in value.keys) {
            out[key.toString()] = _toPlain(value[key]);
          }
          return out;
        }
        if (value is List) {
          return value.map(_toPlain).toList();
        }
        return value;
      }

      class AgentToolsCapability {
        Future<List<Map<String, dynamic>>> list() async {
           final jsonStr = await Bridge.mcpListTools();
           final decoded = dart_convert.jsonDecode(jsonStr) as List;
           return decoded.map((e) => Map<String, dynamic>.from(e as Map)).toList();
        }
        
        Future<dynamic> call(String name, Map<String, dynamic> args) async {
           final jsonStr = await Bridge.mcpCallTool(name, args);
           final decoded = dart_convert.jsonDecode(jsonStr);
           if (decoded is Map) {
              return Map<String, dynamic>.from(decoded);
           }
           return decoded;
        }
      }

      class SysCapability {
        final tools = AgentToolsCapability();

        Future<String> run(String executable, List<String> args, [String? workDir]) {
           return Bridge.sysRun(executable, args, workDir);
        }
        
        Future<bool> confirm(String message) {
           return Bridge.sysConfirm(message);
        }

        dynamic readInput(String prompt) {
           return Bridge.sysReadInput(prompt);
        }

        Future<String> readFile(String path) {
           return Bridge.sysReadFile(path);
        }

        Future<bool> writeFile(String path, String contents) {
           return Bridge.sysWriteFile(path, contents);
        }

        Future<bool> exists(String path) {
           return Bridge.sysExists(path);
        }

        Future<bool> mkdir(String path) {
           return Bridge.sysMkdir(path);
        }

        dynamic jsonDecode(String source) {
           return _toPlain(dart_convert.jsonDecode(source));
        }

        dynamic jsonExtractField(String source, String key) {
           return Bridge.sysJsonExtractField(source, key);
        }

        void print(String? msg) {
           Bridge.sysPrint(msg ?? 'null');
        }
        
        void printMsg(String msg) => this.print(msg);
      }

      class LlmCapability {
        Future<String> chat(String prompt) {
           return Bridge.llmChat(prompt);
        }
      }

      class NetworkCapability {
        Future<String> fetch(String url) {
           return Bridge.netFetch(url);
        }
      }
      
      Map<String, dynamic> _buildContext(List<dynamic> hostArgs) {
         return {
           'args': hostArgs, 
           'capabilities': {
             'sys': SysCapability(),
             'llm': LlmCapability(),
             'net': NetworkCapability()
           }
         };
      }
    ''';

    final fullSource = _normalizeRawStrings('$prelude\n$sourceCode');

    try {
      final program = compiler.compile({
        'agent': {'main.dart': fullSource}
      });

      final runtime = Runtime.ofProgram(program);
      runtime.debugTraceArgs = true;
      for (final plugin in plugins) {
        plugin.configureForRuntime(runtime);
      }

      runtime.registerBridgeFunc('package:agent/main.dart', 'Bridge.sysRun',
          (rt, target, args) {
        String unwrapString(dynamic value) {
          if (value is $Value) {
            final raw = value.$value;
            if (raw is String) return raw;
            final reified = value.$reified;
            if (reified is String) return reified;
            return reified?.toString() ?? raw?.toString() ?? '';
          }
          return value?.toString() ?? '';
        }

        String? unwrapStringOrNull(dynamic value) {
          if (value == null) return null;
          final text = unwrapString(value);
          return text.isEmpty ? null : text;
        }

        List<String> unwrapStringList(dynamic value) {
          if (value is $Value) {
            final reified = value.$reified;
            if (reified is List) {
              return reified.map((e) => e.toString()).toList();
            }
          }
          if (value is List) {
            return value.map((e) => e.toString()).toList();
          }
          return [];
        }

        final executable = unwrapString(args[0]);
        final runArgs = unwrapStringList(args[1]);
        // args[2] corresponds to workDir in the Bridge definition
        final workDir = unwrapStringOrNull(args[2]);
        final future = sys.run(executable, runArgs, workDir: workDir);
        final wrapped = $Future.wrap(future.then((s) => $String(s)));
        return wrapped;
      });

      // ... (rest of your registration code remains the same) ...
      runtime.registerBridgeFunc('package:agent/main.dart', 'Bridge.sysConfirm',
          (rt, target, args) {
        final message = args[0] is $Value
            ? (args[0] as $Value).$value as String
            : args[0] as String;
        final future = sys.confirm(message);
        return $Future.wrap(future.then((v) => $bool(v)));
      });

      runtime.registerBridgeFunc(
          'package:agent/main.dart', 'Bridge.sysReadInput', (rt, target, args) {
        final prompt = args[0] is $Value
            ? (args[0] as $Value).$value as String
            : args[0] as String;
        return $String(sys.readInput(prompt));
      });

      runtime.registerBridgeFunc(
          'package:agent/main.dart', 'Bridge.sysReadFile', (rt, target, args) {
        final path = args[0] is $Value
            ? (args[0] as $Value).$value as String
            : args[0] as String;
        final future = sys.readFile(path);
        return $Future.wrap(future.then((s) => $String(s)));
      });

      runtime.registerBridgeFunc(
          'package:agent/main.dart', 'Bridge.sysWriteFile', (rt, target, args) {
        final path = args[0] is $Value
            ? (args[0] as $Value).$value as String
            : args[0] as String;
        final contents = args[1] is $Value
            ? (args[1] as $Value).$value as String
            : args[1] as String;
        final future = sys.writeFile(path, contents);
        return $Future.wrap(future.then((v) => $bool(v)));
      });

      runtime.registerBridgeFunc('package:agent/main.dart', 'Bridge.sysExists',
          (rt, target, args) {
        final path = args[0] is $Value
            ? (args[0] as $Value).$value as String
            : args[0] as String;
        final future = sys.exists(path);
        return $Future.wrap(future.then((v) => $bool(v)));
      });

      runtime.registerBridgeFunc('package:agent/main.dart', 'Bridge.sysMkdir',
          (rt, target, args) {
        final path = args[0] is $Value
            ? (args[0] as $Value).$value as String
            : args[0] as String;
        final future = sys.mkdir(path);
        return $Future.wrap(future.then((v) => $bool(v)));
      });

      runtime
          .registerBridgeFunc('package:agent/main.dart', 'Bridge.sysJsonDecode',
              (rt, target, args) {
        $Value wrap(dynamic value) {
          if (value == null) return $null();
          if (value is String) return $String(value);
          if (value is int) return $int(value);
          if (value is double) return $double(value);
          if (value is bool) return $bool(value);
          if (value is List) {
            return $List.wrap(value.map(wrap).toList());
          }
          if (value is Map) {
            return $Map.wrap(value.map((k, v) => MapEntry(wrap(k), wrap(v))));
          }
          return $String(value.toString());
        }

        final source = args[0] is $Value
            ? (args[0] as $Value).$value as String
            : args[0] as String;
        // Use Future.sync to handle both synchronous and asynchronous results if SysCapability changes
        // But jsonDecodeValue is currently synchronous in SysCapability.
        // However, the bridge definition says Future<dynamic>.
        // We will wrap result in Future.
        try {
          final result = sys.jsonDecodeValue(source);
          return wrap(result);
        } catch (e) {
          throw e;
        }
      });

      runtime.registerBridgeFunc(
          'package:agent/main.dart', 'Bridge.sysJsonExtractField',
          (rt, target, args) {
        $Value wrap(dynamic value) {
          if (value == null) return $null();
          if (value is String) return $String(value);
          if (value is int) return $int(value);
          if (value is double) return $double(value);
          if (value is bool) return $bool(value);
          if (value is List) {
            return $List.wrap(value.map(wrap).toList());
          }
          if (value is Map) {
            return $Map.wrap(value.map((k, v) => MapEntry(wrap(k), wrap(v))));
          }
          return $String(value.toString());
        }

        String unwrapString(dynamic value) {
          if (value is $Value) {
            final raw = value.$value;
            if (raw is String) return raw;
            final reified = value.$reified;
            if (reified is String) return reified;
            return reified?.toString() ?? raw?.toString() ?? '';
          }
          return value?.toString() ?? '';
        }

        final source = unwrapString(args[0]);
        final key = unwrapString(args[1]);
        final result = sys.jsonExtractField(source, key);
        return wrap(result);
      });

      runtime.registerBridgeFunc('package:agent/main.dart', 'Bridge.sysPrint',
          (rt, target, args) {
        String coerceString(dynamic value) {
          if (value is $Value) {
            final raw = value.$value;
            if (raw is String) return raw;
            final reified = value.$reified;
            if (reified is String) return reified;
            return reified?.toString() ?? raw?.toString() ?? '';
          }
          if (value is String) return value;
          return value?.toString() ?? '';
        }

        if (args.isEmpty) {
          sys.printMsg('null');
          return null;
        }
        final a0 = args[0];
        sys.printMsg(coerceString(a0));
        return null;
      });

      runtime.registerBridgeFunc('package:agent/main.dart', 'Bridge.llmChat',
          (rt, target, args) {
        String unwrapString(dynamic value) {
          if (value is $Value) {
            final raw = value.$value;
            if (raw is String) return raw;
            final reified = value.$reified;
            if (reified is String) return reified;
            return reified?.toString() ?? raw?.toString() ?? '';
          }
          if (value is String) return value;
          return value?.toString() ?? '';
        }

        if (args.isNotEmpty) {
          final a0 = args[0];
          final rawType = a0 is $Value
              ? (a0 as $Value).$value?.runtimeType
              : a0?.runtimeType;
          sys.printMsg(
              '[bridge] llmChat arg0 type: ${a0.runtimeType}, raw type: $rawType');
        }

        final prompt = unwrapString(args.isNotEmpty ? args[0] : null);
        final future = llm.chat(prompt);
        return $Future.wrap(future.then((s) => $String(s)));
      });

      runtime.registerBridgeFunc('package:agent/main.dart', 'Bridge.netFetch',
          (rt, target, args) {
        String unwrapString(dynamic value) {
          if (value is $Value) {
            final raw = value.$value;
            if (raw is String) return raw;
            final reified = value.$reified;
            if (reified is String) return reified;
            return reified?.toString() ?? raw?.toString() ?? '';
          }
          if (value is String) return value;
          return value?.toString() ?? '';
        }

        if (args.isNotEmpty) {
          final a0 = args[0];
          final rawType = a0 is $Value
              ? (a0 as $Value).$value?.runtimeType
              : a0?.runtimeType;
          sys.printMsg(
              '[bridge] netFetch arg0 type: ${a0.runtimeType}, raw type: $rawType');
        }

        final url = unwrapString(args.isNotEmpty ? args[0] : null);
        final future = net.fetch(url);
        return $Future.wrap(future.then((s) => $String(s)));
      });

      runtime.registerBridgeFunc(
          'package:agent/main.dart', 'Bridge.mcpListTools', (rt, target, args) {
        final future = mcp.listTools();
        return $Future.wrap(future.then((list) {
          return $String(jsonEncode(list));
        }));
      });

      runtime.registerBridgeFunc(
          'package:agent/main.dart', 'Bridge.mcpCallTool', (rt, target, args) {
        final name = args[0] is $Value
            ? (args[0] as $Value).$value as String
            : args[0] as String;
        final toolArgs = args[1] is $Value
            ? (args[1] as $Value).$reified as Map
            : args[1] as Map;

        final future = mcp.callTool(name, toolArgs.cast<String, dynamic>());
        return $Future.wrap(future.then((val) {
          return $String(jsonEncode(val));
        }));
      });

      // --- Execute ---
      final boxedArgs = args.map((a) => $String(a)).toList();
      final contextResult = runtime
          .executeLib('package:agent/main.dart', '_buildContext', [boxedArgs]);

      if (contextResult is! $Value) {
        throw Exception("Context build failed.");
      }

      final result = runtime
          .executeLib('package:agent/main.dart', 'main', [contextResult]);

      if (result is $Future) {
        await result.$value;
      }
    } on CompileError catch (e) {
      var message = e.message;
      var line = -1;

      if (e.node != null || e.offset != null) {
        final offset = e.offset ?? e.node!.offset;
        final preludeLines = '\n'.allMatches(prelude).length +
            1; // +1 for the \n between prelude and source

        // Find line number in fullSource
        var fullLine = 1;
        for (var i = 0; i < fullSource.length; i++) {
          if (i == offset) break;
          if (fullSource[i] == '\n') {
            fullLine++;
          }
        }

        if (fullLine > preludeLines) {
          line = fullLine - preludeLines;
          message = "$message at line $line";
        } else {
          message = "$message (in prelude)";
        }
      }

      throw Exception("Sandbox Compilation Error: $message");
    } catch (e, st) {
      // Attempt to adjust stack trace for runtime exceptions
      var stackStr = st.toString();
      var errorStr = e.toString();
      try {
        final preludeLines = '\n'.allMatches(prelude).length + 1;

        String adjustOffsets(String input) {
          final lines = input.split('\n');
          for (var i = 0; i < lines.length; i++) {
            final line = lines[i];
            if (line.contains('package:agent/main.dart:')) {
              final regex = RegExp(r'package:agent/main.dart:(\d+)');
              final match = regex.firstMatch(line);
              if (match != null) {
                final rawOffset = int.parse(match.group(1)!);
                // We need to convert this raw offset to a line number in user code
                if (rawOffset > prelude.length) {
                  var userOffset = rawOffset -
                      prelude.length -
                      1; // -1 for newline separator
                  // Find line number for this userOffset in sourceCode
                  var lineNum = 1;
                  for (var k = 0;
                      k < userOffset && k < sourceCode.length;
                      k++) {
                    if (sourceCode[k] == '\n') lineNum++;
                  }
                  lines[i] = line.replaceFirst(
                      match.group(0)!, 'package:agent/main.dart:$lineNum');
                }
              }
            }
          }
          return lines.join('\n');
        }

        stackStr = adjustOffsets(stackStr);
        errorStr = adjustOffsets(errorStr);
      } catch (_) {}

      throw Exception("Sandbox Error: $errorStr\n$stackStr");
    } finally {
      await mcp.stopAll();
    }
  }
}
