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

    // Declare the Bridge class used to funnel host calls from the sandboxed
    // code. This registers the external static method so dart_eval can resolve
    // it during compilation.
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

    // Prelude: Bridge methods map directly to host capabilities
    final prelude = '''
      import 'dart:async';
      import 'dart:convert';
      
      class Bridge {
         external static Future<String> sysRun(String executable, List<String> args, String? workDir);
         external static Future<bool> sysConfirm(String message);
         external static dynamic sysReadInput(String prompt);
         external static void sysPrint(String message);
         external static Future<String> llmChat(String prompt);
         external static Future<String> netFetch(String url);
         external static Future<String> mcpListTools();
         external static Future<String> mcpCallTool(String name, Map<String, dynamic> args);
      }
      
      class AgentToolsCapability {
        Future<List<Map<String, dynamic>>> list() async {
           final jsonStr = await Bridge.mcpListTools();
           final decoded = jsonDecode(jsonStr);
           return (decoded as List).cast<Map<String, dynamic>>();
        }
        
        Future<dynamic> call(String name, Map<String, dynamic> args) async {
           final jsonStr = await Bridge.mcpCallTool(name, args);
           return jsonDecode(jsonStr);
        }
      }

      class SysCapability {
        final tools = AgentToolsCapability();

        Future<String> run(String executable, List<String> args, {String? workDir}) {
           return Bridge.sysRun(executable, args, workDir);
        }
        
        Future<bool> confirm(String message) {
           return Bridge.sysConfirm(message);
        }

        dynamic readInput(String prompt) {
           return Bridge.sysReadInput(prompt);
        }
        
        void print(String? msg) {
           Bridge.sysPrint(msg ?? 'null');
        }
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
      
      // Public Context Builder
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

      // Bridge each capability call directly to the host
      runtime.registerBridgeFunc('package:agent/main.dart', 'Bridge.sysRun',
          (rt, target, args) {
        print('HOST: Bridge.sysRun called');
        final executable = args[0] is $Value
            ? (args[0] as $Value).$value as String
            : args[0] as String;
        final runArgs = args[1] is $Value
            ? ((args[1] as $Value).$reified as List).cast<String>()
            : (args[1] as List).cast<String>();
        final workDir = args[2] is $Value
            ? (args[2] as $Value).$value as String?
            : args[2] as String?;
        final future = sys.run(executable, runArgs, workDir: workDir);
        final wrapped = $Future.wrap(future.then((s) => $String(s)));
        print('HOST: Bridge.sysRun returning wrapped future: \$wrapped');
        return wrapped;
      });

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

      runtime.registerBridgeFunc('package:agent/main.dart', 'Bridge.sysPrint',
          (rt, target, args) {
        final message = args[0] is $Value
            ? (args[0] as $Value).$value as String
            : args[0] as String;
        sys.printMsg(message);
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
        // Convert List<Map> to $List
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
        // We're returning dynamic, so we might need to wrap it recursively if complex object
        // For simplicity, assuming JSON primitives or standard collections
        // dart_eval needs better wrapping for complex maps, but $Value.encode logic or similar might be needed.
        // But for now let's hope standard wrapping works for primitives.
        // Actually dart_eval helpers are better manually invoked for deep structure.
        // But we return dynamic.

        return $Future.wrap(future.then((val) {
          return $String(jsonEncode(val));
        }));
      });

      // --- Execute ---
      final boxedArgs = args.map((a) => $String(a)).toList();
      final contextResult = runtime
          .executeLib('package:agent/main.dart', '_buildContext', [boxedArgs]);

      if (contextResult is! $Value) {
        // Fallback for simulation/mock if something goes wrong
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
