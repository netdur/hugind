// ignore: main_first_positional_parameter_type
void main(Map<String, dynamic> context) async {
  final sys = context['capabilities']['sys'];
  final llm = context['capabilities']['llm'];
  final net = context['capabilities']['net'];

  while (true) {
    // 1. INPUT
    // final input = sys.readInput('\n❓ Ask a question (or "exit"): ');
    // final rawQuery = input.toString().trim();
    final rawQuery = "messi"; // keep for testing
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

    // final searchJson = (await net.fetch(url)).toString();
    final searchJson = r'''
{"batchcomplete":"","continue":{"sroffset":3,"continue":"-||"},"query":{"searchinfo":{"totalhits":1956,"suggestion":"lionel mes","suggestionsnippet":"lionel mes"},"search":[{"ns":0,"title":"Lionel Messi","pageid":2150841,"size":317675,"wordcount":25060,"snippet":"<span class=\"searchmatch\">Lionel</span> Andr\u00e9s &quot;Leo&quot; <span class=\"searchmatch\">Messi</span> (born 24 June 1987) is an Argentine professional footballer who plays as a forward for and captains both Major League Soccer","timestamp":"2026-01-27T09:33:55Z"},{"ns":0,"title":"Career of Lionel Messi","pageid":76266203,"size":333701,"wordcount":32149,"snippet":"<span class=\"searchmatch\">Lionel</span> <span class=\"searchmatch\">Messi</span> is an Argentine professional footballer who plays as a forward for and captains both Major League Soccer club Inter Miami and the Argentina","timestamp":"2026-01-27T23:51:30Z"},{"ns":0,"title":"Messi\u2013Ronaldo rivalry","pageid":43992506,"size":156045,"wordcount":9110,"snippet":"football propelled by the media and fans that involves Argentine footballer <span class=\"searchmatch\">Lionel</span> <span class=\"searchmatch\">Messi</span> and Portuguese footballer Cristiano Ronaldo, mainly for being contemporaries","timestamp":"2026-01-27T03:35:09Z"}]}}
''';
    sys.print('📄 Search Results: $searchJson');

    if (searchJson.isEmpty) {
      sys.print("getTitlesFromSearch: empty JSON");
      continue;
    }

    // Get titles
    var titles = extractSearchTitles(searchJson, sys);
    print(titles.toString());
    // var titlesStr = titles.join(', ');
    // sys.print('📚 Titles: ${titles.toString()}');

    // 4. EXTRACT KEY INFO PER PAGE
    // Use recursive helper to avoid loop variable stack issues
    // final keyInfo = <String, String>{};
    // await processTitles(0, titles, keyInfo, context, rawQuery);

    /*
    if (keyInfo.isEmpty) {
      sys.print('❌ No relevant information found.');
      break;
    }

    // 5. SYNTHESIZE ANSWER
    // Pass the key information to llm and ask it to generate a response
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
    break;
  }
}

// Recursive helper to process titles one by one
Future<void> processTitles(
    int index,
    List<String> titles,
    Map<String, String> keyInfo,
    Map<String, dynamic> context,
    String rawQuery) async {
  if (index >= titles.length) return;

  final sys = context['capabilities']['sys'];
  final net = context['capabilities']['net'];
  final llm = context['capabilities']['llm'];
  final title = titles[index];

  sys.print("📄 Reading: $title");
  /*
  final encodedTitle = Uri.encodeComponent(title);
  final contentUrl =
      'https://en.wikipedia.org/w/api.php?action=query&prop=extracts&explaintext&titles=$encodedTitle&format=json';

  String contentJson;
  try {
    contentJson = (await net.fetch(contentUrl)).toString();
  } catch (e) {
    sys.print('❌ Read Error: $e');
    await processTitles(index + 1, titles, keyInfo, context, rawQuery);
    return;
  }

  final extract = extractWikipediaExtract(contentJson, sys);
  if (extract.isEmpty) {
    sys.print('⚠️ Empty extract for $title');
    await processTitles(index + 1, titles, keyInfo, context, rawQuery);
    return;
  }

  sys.print("🤖 Extracting info from $title...");
  // rawQuery is passed as an argument.

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

  await processTitles(index + 1, titles, keyInfo, context, rawQuery);
  */
}

// --- HELPERS ---

List<String> extractSearchTitles(String jsonText, dynamic sys) {
  var titles = <String>[];
  try {
    final decoded = sys.jsonDecode(jsonText) as Map;
    final query = decoded['query'];
    if (query is Map) {
      final search = query['search'];
      if (search is List) {
        for (final item in search) {
          final mapItem = item as Map;
          titles.add("${mapItem['title']}");
        }
      }
    }
    sys.print("Titles: ${titles.length}");
  } catch (e) {
    sys.print("Error parsing titles: $e");
  }
  return titles;
}

String extractWikipediaExtract(String jsonText, dynamic sys) {
  try {
    final decoded = sys.jsonDecode(jsonText) as Map;
    final query = decoded['query'];
    if (query is Map) {
      final pages = query['pages'];
      if (pages is Map && pages.isNotEmpty) {
        final page = pages.values.first as Map;
        if (page['extract'] != null) {
          return page['extract'].toString();
        }
      }
    }
  } catch (e) {
    // silently fail or return empty
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
