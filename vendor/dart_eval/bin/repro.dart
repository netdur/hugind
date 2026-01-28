import 'dart:typed_data';
import 'package:dart_eval/dart_eval.dart';
import 'package:dart_eval/dart_eval_bridge.dart';
import 'package:dart_eval/stdlib/core.dart';

void main() {
  final compiler = Compiler();

  final program = compiler.compile({
    'my_app': {
      'main.dart': '''
        void main() {
          
          var text = "some\\ntext";
          if (text.contains('foo')) {
             text = text.trim();
          }
          final lines = text.split('\\n');
          final candidate = lines.last.trim();
          print(candidate);
          print(candidate.runtimeType);
          if (candidate.isEmpty || candidate.length < 3) {
            print("empty or short");
          } else {
             print("not empty");
          }
        }
      '''
    }
  });

  final runtime = Runtime(ByteData.view(program.write().buffer));
  runtime.registerBridgeFunc('dart:core', 'print', (rt, target, args) {
    print(args[0]?.$value);
    return null;
  });
  // CoreStdlib.configureForRuntime(runtime); // Not needed
  runtime.executeLib('package:my_app/main.dart', 'main');
}
