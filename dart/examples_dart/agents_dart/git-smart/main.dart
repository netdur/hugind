// ignore: main_first_positional_parameter_type
dynamic main(Map<String, dynamic> context) async {
  final sys = context['capabilities']['sys'];
  final llm = context['capabilities']['llm'];

  sys.print('🧠 Smart CLI v0.12.0 (ReadOnly Guard)');
  sys.print('-------------------------------------');

  // --- 1. SYSTEM PROBE ---
  String osInfo = 'POSIX';
  try {
    dynamic uname = await sys.run('uname', ['-a']);
    osInfo = uname.toString().trim();
  } catch (e) {
    sys.print('⚠️ Probe error: $e');
  }
  sys.print('💻 System: ' + osInfo);

  while (true) {
    // --- 2. INPUT ---
    dynamic input = sys.readInput('\n💬 Request (or "exit"): ');
    String sInput = input.toString().trim();

    if (sInput == 'exit') {
      sys.print('👋 Goodbye.');
      break;
    }
    if (sInput == '') continue;

    // --- 3. PROMPT ---
    String prompt = 'SYSTEM CONTEXT:\n' + osInfo + '\n\n' +
        'USER REQUEST: "' + sInput + '"\n\n' +
        'GUIDELINES:\n' +
        '1. **Goal**: Provide the best native shell command.\n' +
        '2. **Discovery**: For apps, check `/Applications` (Mac) or `/usr/bin` (Linux).\n' +
        '3. **Safety**: Append "|| echo \'Not found\'" to searches.\n' +
        '4. **Silence**: Use "2>/dev/null" to hide permission errors.\n\n' +
        'OUTPUT FORMAT:\n' +
        '###THOUGHT###\n(Reasoning)\n' +
        '###COMMAND###\n(The shell command)\n' +
        '###END###';
    
    // Note: We removed the "RISK" tag from the prompt. 
    // We (the code) will decide the risk, not the LLM.

    sys.print('🤔 Thinking...');
    dynamic response = await llm.chat(prompt);
    String respStr = response.toString();

    // --- 4. PARSING ---
    String cmd = '';
    String thought = '';
    
    List<String> lines = respStr.split('\n');
    String section = '';
    
    int i = 0;
    while (i < lines.length) {
      String line = lines[i];
      String trim = line.trim();
      
      if (trim == '###THOUGHT###') {
        section = 'THOUGHT';
      } else if (trim == '###COMMAND###') {
        section = 'CMD';
      } else if (trim == '###END###') {
        section = 'DONE';
      } else {
        if (section == 'THOUGHT') {
           if (thought != '') thought = thought + ' ';
           thought = thought + trim;
        }
        if (section == 'CMD') {
           if (cmd != '') cmd = cmd + ' ';
           cmd = cmd + trim;
        }
      }
      i = i + 1;
    }

    cmd = cmd.trim();
    if (cmd.startsWith('`')) cmd = cmd.replaceAll('`', '');

    if (cmd == '') {
      sys.print('❌ Error: Could not parse command.');
      continue;
    }

    sys.print('💡 Logic: ' + thought);
    sys.print('👉 Command: ' + cmd);

    // --- 5. READ-ONLY GUARD (The Rule) ---
    // Rule: "Is this strictly read-only?"
    
    bool isReadOnly = false;
    String cleanCmd = cmd.toLowerCase().trim();

    // A. Whitelist of Read-Only Tools
    // We check if the command STARTS with these.
    bool toolSafe = false;
    if (cleanCmd.startsWith('ls ')) toolSafe = true;
    if (cleanCmd.startsWith('grep ')) toolSafe = true;
    if (cleanCmd.startsWith('find ')) toolSafe = true;
    if (cleanCmd.startsWith('mdfind ')) toolSafe = true; // Mac Spotlight
    if (cleanCmd.startsWith('locate ')) toolSafe = true;
    if (cleanCmd.startsWith('cat ')) toolSafe = true;
    if (cleanCmd.startsWith('head ')) toolSafe = true;
    if (cleanCmd.startsWith('tail ')) toolSafe = true;
    if (cleanCmd.startsWith('uname')) toolSafe = true;
    if (cleanCmd.startsWith('whoami')) toolSafe = true;
    if (cleanCmd.startsWith('pwd')) toolSafe = true;
    if (cleanCmd.startsWith('which ')) toolSafe = true;
    if (cleanCmd.startsWith('du ')) toolSafe = true;
    if (cleanCmd.startsWith('df ')) toolSafe = true;
    if (cleanCmd.startsWith('echo ')) toolSafe = true;
    if (cleanCmd.startsWith('printf ')) toolSafe = true;
    if (cleanCmd.startsWith('sysctl ')) toolSafe = true;

    // B. Blacklist of Write Operations
    // Even if it uses 'ls', if it redirects (>), it's a write.
    bool opSafe = true;
    if (cleanCmd.contains('>')) opSafe = false;       // Redirect to file
    if (cleanCmd.contains('touch ')) opSafe = false;  // Create file
    if (cleanCmd.contains('rm ')) opSafe = false;     // Delete
    if (cleanCmd.contains('mv ')) opSafe = false;     // Move
    if (cleanCmd.contains('cp ')) opSafe = false;     // Copy
    if (cleanCmd.contains('chmod ')) opSafe = false;  // Perms
    if (cleanCmd.contains('mkdir ')) opSafe = false;  // Create dir
    if (cleanCmd.contains(' | sh')) opSafe = false;   // Pipe to shell execution
    if (cleanCmd.contains(' -exec')) opSafe = false;  // find -exec (dangerous)

    if (toolSafe == true) {
      if (opSafe == true) {
        isReadOnly = true;
      }
    }

    // --- 6. EXECUTION ---
    bool shouldRun = false;
    
    if (isReadOnly) {
       sys.print('✅ Read-Only (Safe). Auto-executing...');
       shouldRun = true;
    } else {
       sys.print('⚠️  Modifies System or Unknown Tool.');
       dynamic confirmed = await sys.confirm('⚡ Execute?');
       if (confirmed == true) shouldRun = true;
       else sys.print('🛑 Cancelled.');
    }

    if (shouldRun) {
      try {
        dynamic res = await sys.run('sh', ['-c', cmd]);
        String output = res.toString().trim();
        
        sys.print('\n📄 OUTPUT:');
        if (output == '') {
          sys.print('(No output)');
        } else {
          sys.print(output);
        }
      } catch (err) {
        sys.print('❌ System Error: ' + err.toString());
      }
    }
  }
}