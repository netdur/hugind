export default async function main(params) {
    const finish = (value) => {
        set_result(value);
    };
    // ----------------------------
    // Helpers
    // ----------------------------
    const cleanInput = (s) =>
        (s || "")
            .trim()
            .replace(/[\/\s]+$/g, "") // remove trailing slashes/spaces
            .replace(/\s+/g, " ");

    const unique = (arr) => Array.from(new Set(arr));

    const extractCitedSourceIds = (text) => {
        const ids = [];
        const re = /\[(\d+)\]/g;
        let m;
        while ((m = re.exec(text)) !== null) ids.push(Number(m[1]));
        return unique(ids).sort((a, b) => a - b);
    };

    const looksLikeAcronym = (q) => {
        // very short / token-ish acronym queries
        const t = q.trim();
        const tokens = t.split(/\s+/);
        if (tokens.length === 1) {
            const one = tokens[0];
            // e.g. SOC, soc, cr7, cpu, gpu, etc.
            return one.length <= 4;
        }
        return false;
    };

    const hasDomainHint = (q) => {
        // common "acronym in X" / "X meaning in Y"
        return /\b(in|for|within|on)\b/i.test(q);
    };

    const isVague = (q) => {
        const t = q.trim();
        if (!t) return true;
        const tokens = t.split(/\s+/);
        if (tokens.length === 1) return true;
        if (looksLikeAcronym(t)) return true;
        return false;
    };

    // ----------------------------
    // 1) Get input
    // ----------------------------
    const rawQuery = cleanInput(await input("Enter your question: "));
    print("User Question: " + rawQuery);

    if (!rawQuery) {
        print("Please enter a question.");
        return finish({});
    }

    // ----------------------------
    // 2) Refine query (improved: expand acronyms when domain exists)
    // ----------------------------
    print("Refining query...");

    const refinePrompt = `
SYSTEM: Rewrite the user query into the best possible Wikipedia search query.

Rules:
- Prefer official Wikipedia article titles when possible.
- Remove filler words and punctuation that doesn't change meaning.
- If the query contains an acronym AND a domain hint (e.g. "in computers", "for security"),
  expand the acronym to the most likely full form in that domain, using the canonical Wikipedia title.
- Output ONLY the final query text (no quotes, no extra lines).

Examples:
User: "soc in computers"
Output: "System on a chip"

User: "cr7"
Output: "Cristiano Ronaldo"

User: "what is soc?"
Output: "SOC"

User: "messi"
Output: "Lionel Messi"

User query: ${rawQuery}
Final query:
`;

    let refinedQuery = rawQuery;
    try {
        refinedQuery = (await llm.chat(refinePrompt)).trim().replace(/^"|"$/g, "");
    } catch (e) {
        // If refine fails, fall back to raw query (don’t hard-fail)
        refinedQuery = rawQuery;
    }

    refinedQuery = cleanInput(refinedQuery);
    print("Refined Query: " + refinedQuery);

    // ----------------------------
    // 3) Wikipedia search (top 3)
    // ----------------------------
    print("Searching Wikipedia...");

    const searchUrl =
        `https://en.wikipedia.org/w/api.php?action=query&list=search&srsearch=${encodeURIComponent(refinedQuery)}&format=json&srlimit=3`;

    let searchResults = [];
    try {
        const jsonStr = await net.fetch(searchUrl);
        const data = JSON.parse(jsonStr);
        searchResults = data?.query?.search ?? [];
    } catch (e) {
        print("Error fetching Wikipedia search: " + e);
        return finish({});
    }

    if (searchResults.length === 0) {
        const fallback = (await llm.chat(`
SYSTEM: No Wikipedia results were found.
Respond with:
1) A brief apology
2) 3 alternative search queries the user could try (Wikipedia-style titles)
Keep it short.
User question: ${rawQuery}
`)).trim();
        print(fallback);
        return finish({});
    }

    const titles = searchResults.map(r => r.title);
    print("Top results: " + titles.join(" | "));

    // ----------------------------
    // 4) Fetch short intros for all titles in ONE request
    // ----------------------------
    const extractsUrl =
        `https://en.wikipedia.org/w/api.php?action=query&prop=extracts&exintro=1&explaintext=1&exsentences=6&titles=${encodeURIComponent(titles.join("|"))}&format=json&redirects=1`;

    let pages = [];
    try {
        const extractJson = await net.fetch(extractsUrl);
        const extractData = JSON.parse(extractJson);
        const pageMap = extractData?.query?.pages ?? {};

        pages = Object.keys(pageMap)
            .map(id => pageMap[id])
            .filter(p => p && p.title && p.extract && p.extract.trim().length > 0)
            .map(p => ({ title: p.title.trim(), extract: p.extract.trim() }));
    } catch (e) {
        print("Error fetching Wikipedia extracts: " + e);
        return finish({});
    }

    if (pages.length === 0) {
        print("Wikipedia returned results, but no extractable intro text was found.");
        return finish({});
    }

    // Keep at most 3 sources (align with your search limit)
    pages = pages.slice(0, 3);

    // ----------------------------
    // 5) Build numbered sources block (for reliable citations)
    // ----------------------------
    const sourcesText = pages
        .map((p, i) => `[${i + 1}] ${p.title}\n${p.extract}`)
        .join("\n\n---\n\n");

    const availableSourceIds = pages.map((_, i) => `[${i + 1}]`).join(" ");

    // ----------------------------
    // 6) Decide: answer mode vs clarify mode (better UX for acronyms/ambiguous)
    // ----------------------------
    const disambigHit =
        pages.some(p => /\(disambiguation\)/i.test(p.title)) ||
        titles.some(t => /\(disambiguation\)/i.test(t));

    const ambiguous =
        disambigHit || looksLikeAcronym(rawQuery) || (isVague(rawQuery) && !hasDomainHint(rawQuery));

    // ----------------------------
    // 7) Single LLM call (stricter “explicit-only” facts + citations)
    // ----------------------------
    print("Synthesizing answer...");

    const systemRules = `
SYSTEM:
You answer using ONLY the provided sources.

Hard rules:
- Only state facts that are explicitly mentioned in the source text.
- Do NOT add numbers, awards, dates, counts, or achievements unless they appear verbatim in the sources.
- Every factual claim MUST end with a citation like [1] or [1][2].
- Use ONLY these source numbers: ${availableSourceIds}.
- Do NOT invent citations or sources.
- If something is not explicitly stated in the sources, say: "Not stated in the sources."
- Keep the answer concise. Avoid long lists unless the user asks for them.
`;

    let answerPrompt = "";
    if (ambiguous) {
        // Clarify mode: list a few meanings, then ask one follow-up
        answerPrompt = `
${systemRules}
Task:
- The user query is ambiguous (often an acronym or short term).
- List 2–4 possible meanings from the sources, each as a short bullet with citations.
- Then ask ONE clarifying question about the intended context (e.g., computing, biology, sports, security).

User question: ${rawQuery}

Sources:
${sourcesText}

Answer:
`;
    } else {
        // Answer mode: short direct answer + one follow-up if useful
        answerPrompt = `
${systemRules}
Task:
- Answer the user's question directly using the sources.
- If the question is still broad, add ONE helpful follow-up question at the end.

User question: ${rawQuery}

Sources:
${sourcesText}

Answer:
`;
    }

    let finalAnswer = "";
    try {
        finalAnswer = (await llm.chat(answerPrompt)).trim();
    } catch (e) {
        print("Error generating answer: " + e);
        return finish({});
    }

    print("\n=== FINAL ANSWER ===\n");
    print(finalAnswer);

    // ----------------------------
    // 8) Print only sources that were actually cited (cleaner UX)
    // ----------------------------
    const usedIds = extractCitedSourceIds(finalAnswer);
    const used = usedIds.length ? usedIds : pages.map((_, i) => i + 1); // fallback if model forgets citations

    print("\n=== SOURCES ===\n");
    used.forEach((id) => {
        const p = pages[id - 1];
        if (p) print(`[${id}] ${p.title}`);
    });

    return finish({});
}
