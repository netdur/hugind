import 'package:analyzer/dart/ast/ast.dart';
import 'package:dart_eval/src/eval/compiler/context.dart';
import 'package:dart_eval/src/eval/compiler/errors.dart';
import 'package:dart_eval/src/eval/compiler/model/label.dart';
import 'package:dart_eval/src/eval/compiler/statement/statement.dart';

StatementInfo compileContinueStatement(
    ContinueStatement s, CompilerContext ctx) {
  if (s.label != null) {
    throw CompileError('Continue labels are not currently supported', s);
  }

  final currentState = ctx.saveState();

  final index = ctx.labels
      .lastIndexWhere((label) => label.type == LabelType.loopContinue);
  if (index == -1) {
    throw CompileError(
        'Cannot use \'continue\' outside of a loop context', s);
  }

  for (var i = ctx.labels.length - 1; i > index; i--) {
    if (ctx.labels[i].type == LabelType.loop) {
      continue;
    }
    ctx.labels[i].cleanup(ctx);
  }

  final label = ctx.labels[index];
  final offset = label.cleanup(ctx);
  if (!ctx.labelReferences.containsKey(label)) {
    ctx.labelReferences[label] = {};
  }
  ctx.labelReferences[label]!.add(offset);
  ctx.restoreState(currentState);
  return StatementInfo(-1);
}
