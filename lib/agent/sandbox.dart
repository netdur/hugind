import 'dart:async';
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

  AgentSandbox(this.sys, this.llm);

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
        },
        bridge: true));

    // Prelude: Bridge methods map directly to host capabilities
    final prelude = '''
      import 'dart:async';
      
      class Bridge {
         external static Future<String> sysRun(String executable, List<String> args, String? workDir);
         external static Future<bool> sysConfirm(String message);
         external static void sysPrint(String message);
         external static Future<String> llmChat(String prompt);
      }
      
      class SysCapability {
        Future<String> run(String executable, List<String> args, {String? workDir}) async {
           var res = await Bridge.sysRun(executable, args, workDir);
           return res.toString();
        }
        
        Future<bool> confirm(String message) async {
           return await Bridge.sysConfirm(message);
        }
        
        void print(String? msg) {
          Bridge.sysPrint(msg ?? 'null');
        }
        
        // Allowed paths logic typically happens on Host for security, 
        // avoiding putting security logic in the Sandbox itself.
      }

      class LlmCapability {
        Future<String> chat(String prompt) async {
           var res = await Bridge.llmChat(prompt);
           return res.toString();
        }
      }
      
      // Public Context Builder
      Map<String, dynamic> _buildContext() {
         return {
           'args': <String>[], 
           'capabilities': {
             'sys': SysCapability(),
             'llm': LlmCapability()
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
        return $Future.wrap(future.then((s) => $String(s)));
      });

      runtime.registerBridgeFunc('package:agent/main.dart', 'Bridge.sysConfirm',
          (rt, target, args) {
        final message = args[0] is $Value
            ? (args[0] as $Value).$value as String
            : args[0] as String;
        final future = sys.confirm(message);
        return $Future.wrap(future.then((v) => $bool(v)));
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

      // --- Execute ---
      final contextResult =
          runtime.executeLib('package:agent/main.dart', '_buildContext', []);

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
    }
  }
}
