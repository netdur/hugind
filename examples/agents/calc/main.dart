// ignore: main_first_positional_parameter_type
dynamic main(Map<String, dynamic> context) async {
  var sys = context['capabilities']['sys'];
  var llm = context['capabilities']['llm'];
  
  sys.print("Simple Math Agent started. Type 'quit' to exit.");
  
  while (true) {
    var input = sys.readInput("Enter a math expression: ");
    if (input.trim().toLowerCase() == 'quit') {
      sys.print("Goodbye!");
      break;
    }
    
    try {
      var result = await llm.chat("Calculate: $input");
      sys.print("Result: $result");
    } catch (e) {
      sys.print("Error: $e");
    }
  }
}