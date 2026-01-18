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
import 'capabilities.dart';

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
      import 'dart:convert';
      
      class Bridge {
         external static Future<String> sysRun(String executable, List<String> args, String? workDir);
         external static Future<bool> sysConfirm(String message);
         external static dynamic sysReadInput(String prompt);
         external static Future<String> sysReadFile(String path);
         external static Future<bool> sysWriteFile(String path, String contents);
         external static Future<bool> sysExists(String path);
         external static Future<bool> sysMkdir(String path);
         external static void sysPrint(String message);
         external static Future<String> llmChat(String prompt);
         external static Future<String> netFetch(String url);
         external static Future<String> mcpListTools();
         external static Future<String> mcpCallTool(String name, Map<String, dynamic> args);
      }
      
      class AgentToolsCapability {
        Future<List<Map<String, dynamic>>> list() async {
           final jsonStr = await Bridge.mcpListTools();
           final decoded = jsonDecode(jsonStr) as List;
           return decoded.map((e) => Map<String, dynamic>.from(e as Map)).toList();
        }
        
        Future<dynamic> call(String name, Map<String, dynamic> args) async {
           final jsonStr = await Bridge.mcpCallTool(name, args);
           final decoded = jsonDecode(jsonStr);
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

    final fullSource = '$prelude\n$sourceCode';

    try {
      final program = compiler.compile({
        'agent': {'main.dart': fullSource}
      });

      final runtime = Runtime.ofProgram(program);
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

      runtime.registerBridgeFunc('package:agent/main.dart', 'Bridge.sysReadFile',
          (rt, target, args) {
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

        sys.printMsg(coerceString(args[0]));
        return null;
      });

      runtime.registerBridgeFunc('package:agent/main.dart', 'Bridge.llmChat',
          (rt, target, args) {
        final prompt = args[0] is $Value
            ? (args[0] as $Value).$value as String
            : args[0] as String;
        final future = llm.chat(prompt);
        return $Future.wrap(future.then((s) => $String(s)));
      });

      runtime.registerBridgeFunc('package:agent/main.dart', 'Bridge.netFetch',
          (rt, target, args) {
        final url = args[0] is $Value
            ? (args[0] as $Value).$value as String
            : args[0] as String;
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
    } catch (e, st) {
      throw Exception("Sandbox Error: $e\n$st");
    } finally {
      await mcp.stopAll();
    }
  }
}