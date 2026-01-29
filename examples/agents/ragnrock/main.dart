// ignore: main_first_positional_parameter_type
void main(Map<String, dynamic> context) async {
  final sys = context['capabilities']['sys'];
  final llm = context['capabilities']['llm'];
  final net = context['capabilities']['net'];

  while (true) {
    // 1. INPUT
    final input = sys.readInput('\n❓ Ask a question (or "exit"): ');
    final rawQuery = input.toString().trim();
    if (rawQuery.toLowerCase() == 'exit') break;
    if (rawQuery.isEmpty) continue;

    // 2. REFINE QUERY
    sys.print('🧠 Refining query...');
    final refinePrompt = '''
SYSTEM: Convert a raw user query into a clean, canonical search query suitable for a general-purpose search engine.
Rules:
Resolve the query to the most relevant entity, concept, or topic
Remove conversational language, filler words, and ambiguity
Prefer official names or widely accepted terms
Normalize implied roles, relationships, or attributes into standard phrasing
Do not add explanations, commentary, or formatting
Output only the finalized search query
User: $rawQuery
Output (query only):
''';
    final refinedRes = await llm.chat(refinePrompt);
    sys.print('🔍 Refined Query: $refinedRes');
    final searchTerm = cleanLlmResponse(refinedRes.toString());

    // 3. SEARCH
    final encodedQuery = Uri.encodeComponent(searchTerm);
    final url =
        'https://en.wikipedia.org/w/api.php?action=query&list=search&srsearch=$encodedQuery&format=json&srlimit=3';
    sys.print('🌐 Searching Wikipedia for: "$searchTerm"... $url');

    final searchJson = (await net.fetch(url)).toString();

    if (searchJson.isEmpty) {
      sys.print("getTitlesFromSearch: empty JSON");
      continue;
    }

    // Get titles using robust native extraction (inlined)
    final query = sys.jsonExtractField(searchJson, 'query');
    final search = sys.jsonExtractField(query, 'search');
    final titles = sys.jsonExtractField(search, 'title');

    // Check if titles is a valid list
    if (titles is! List || titles.isEmpty) {
      sys.print('❌ No relevant articles found.');
      continue;
    }

    // Manual join to avoid List cast issues
    final sb = StringBuffer();
    for (final t in titles) {
      if (sb.isNotEmpty) sb.write(', ');
      sb.write(t);
    }
    sys.print('📚 Titles: ${sb.toString()}');

    /*
    // 4. EXTRACT KEY INFO PER PAGE
    final keyInfo = <String, String>{};
    for (final title in titles) {
      await processTitle(title.toString(), keyInfo, context, rawQuery);
    }

    if (keyInfo.isEmpty) {
      sys.print('❌ No relevant information found.');
      continue;
    }

    // 5. SYNTHESIZE ANSWER
    sys.print('🧠 Synthesizing final answer...');
    final synthesisPrompt = '''
SYSTEM: Answer the user question based on the provided extracted information.
User Query: $rawQuery

Extracted Information:
${keyInfo.entries.map((e) => "SOURCE: ${e.key}\nFACTS:\n${e.value}").join('\n\n')}

Output (Final Answer):
''';

    final finalAnswer = await llm.chat(synthesisPrompt);
    sys.print('\n📝 Final Answer:\n$finalAnswer');
    */
  }
}

Future<void> processTitle(String title, Map<String, String> keyInfo,
    Map<String, dynamic> context, String rawQuery) async {
  final sys = context['capabilities']['sys'];
  final net = context['capabilities']['net'];
  final llm = context['capabilities']['llm'];

  sys.print("📄 Reading: $title");

  final encodedTitle = Uri.encodeComponent(title);
  final contentUrl =
      'https://en.wikipedia.org/w/api.php?action=query&prop=extracts&explaintext&titles=$encodedTitle&format=json';

  String contentJson;
  try {
    contentJson = (await net.fetch(contentUrl)).toString();
  } catch (e) {
    sys.print('❌ Read Error for $title: $e');
    return;
  }

  final extract = extractWikipediaExtract(contentJson, sys);
  if (extract.isEmpty) {
    sys.print('⚠️ Empty extract for $title');
    return;
  }

  sys.print("🤖 Extracting info from $title...");

  final extractionPrompt = '''
SYSTEM: Extract specific facts and key information relevant to the user query from the provided text.
User Query: $rawQuery
Text:
${truncate(extract, 4000)}

Output (Bullet points of key facts only):
''';
  final extractedInfo = await llm.chat(extractionPrompt);
  keyInfo[title] = extractedInfo;
  sys.print('✅ Extracted facts from $title');
}

// --- HELPERS ---

String extractWikipediaExtract(String jsonText, dynamic sys) {
  try {
    // Robust extraction: query -> pages -> * -> extract
    final query = sys.jsonExtractField(jsonText, 'query');
    final pages = sys.jsonExtractField(query, 'pages');

    // pages is a valid JSON string (of a map) if successful.
    // We can't iterate keys easily in dart_eval due to bugs.
    // But we can try to decode it as a Map and take values.first.
    // If that fails, we might need a `jsonExtractFirstValue` capability, but let's try standard decode first.
    final decodedPages = sys.jsonDecode(pages) as Map;
    if (decodedPages.isNotEmpty) {
      final page = decodedPages.values.first as Map;
      if (page['extract'] != null) {
        return page['extract'].toString();
      }
    }
  } catch (e) {
    // fail silently
  }
  return '';
}

String cleanLlmResponse(String response) {
  String text = response.trim();
  if (text.contains('```')) text = text.replaceAll('```', '').trim();
  return text;
}

String truncate(dynamic text, int limit) {
  if (text == null) return "";
  if (text is! String) return text.toString();
  if (text.length <= limit) return text;
  return text.substring(0, limit) + "...";
}
