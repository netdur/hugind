// ./target/release/hugind agent run agent/doc_parser --input /path/to/file.pdf

function parseArgs(input) {
    const args = (input && Array.isArray(input.args)) ? input.args.slice() : [];
    const out = {
        input: ""
    };

    for (let i = 0; i < args.length; i += 1) {
        const key = args[i];
        if (key === "--input" || key === "--uri" || key === "-i") {
            out.input = args[i + 1] || "";
            i += 1;
        } else if (!out.input && key && !key.startsWith("--")) {
            out.input = key;
        }
    }

    return out;
}

function toUri(value) {
    if (!value) return "";
    if (value.startsWith("http://") || value.startsWith("https://")) return value;
    if (value.startsWith("file:") || value.startsWith("data:")) return value;
    const real = fs.realpath(value);
    return `file://${real}`;
}

async function resolveToolName() {
    const listJson = await tools.list();
    const list = JSON.parse(listJson);
    if (!Array.isArray(list) || list.length === 0) {
        return "";
    }

    let exact = "";
    let fallback = "";
    for (const tool of list) {
        if (!tool || !tool.name) continue;
        if (tool.name === "markitdown:convert_to_markdown") {
            exact = tool.name;
        }
        if (!fallback && tool.name.endsWith(":convert_to_markdown")) {
            fallback = tool.name;
        }
        if (!fallback && tool.name === "convert_to_markdown") {
            fallback = tool.name;
        }
    }
    return exact || fallback || "";
}

function buildStructuringPrompt(markdown) {
    return [
        "You are an information extraction engine.",
        "Extract structured data from the document below.",
        "First, infer a document_type from the content.",
        "If the document looks like an invoice/receipt, fill the invoice section.",
        "Otherwise, leave invoice fields null and fill generic sections.",
        "Return ONLY valid JSON. No markdown or extra text.",
        "Schema:",
        "{",
        "  \"document_type\": string,",
        "  \"summary\": string | null,",
        "  \"entities\": [",
        "    { \"label\": string, \"value\": string }",
        "  ],",
        "  \"key_values\": [",
        "    { \"key\": string, \"value\": string }",
        "  ],",
        "  \"tables\": [",
        "    { \"title\": string | null, \"rows\": [[string]] }",
        "  ],",
        "  \"sections\": [",
        "    { \"heading\": string | null, \"content\": string }",
        "  ],",
        "  \"invoice\": {",
        "    \"invoice_number\": string | null,",
        "    \"invoice_date\": string | null,",
        "    \"due_date\": string | null,",
        "    \"currency\": string | null,",
        "    \"total\": string | null,",
        "    \"subtotal\": string | null,",
        "    \"tax_total\": string | null,",
        "    \"vendor\": {",
        "      \"name\": string | null,",
        "      \"address\": string | null,",
        "      \"email\": string | null,",
        "      \"phone\": string | null",
        "    },",
        "    \"bill_to\": {",
        "      \"name\": string | null,",
        "      \"address\": string | null",
        "    },",
        "    \"line_items\": [",
        "      {",
        "        \"description\": string | null,",
        "        \"quantity\": string | null,",
        "        \"unit_price\": string | null,",
        "        \"amount\": string | null",
        "      }",
        "    ],",
        "    \"notes\": string | null,",
        "    \"bank_details\": {",
        "      \"bank_name\": string | null,",
        "      \"swift\": string | null,",
        "      \"bank_code\": string | null,",
        "      \"local_code\": string | null,",
        "      \"account_code\": string | null,",
        "      \"rib_code\": string | null",
        "    }",
        "  }",
        "}",
        "",
        "Document:",
        markdown
    ].join("\n");
}

export default async function main(input) {
    const opts = parseArgs(input);
    if (!opts.input) {
        const msg = "Missing input. Use --input <path|url>.";
        set_result({ error: msg });
        print(JSON.stringify({ error: msg }, null, 2));
        return;
    }

    let uri;
    try {
        uri = toUri(opts.input);
    } catch (e) {
        const err = { error: "Failed to resolve input path.", details: String(e) };
        set_result(err);
        print(JSON.stringify(err, null, 2));
        return;
    }

    let toolName = "";
    try {
        toolName = await resolveToolName();
    } catch (e) {
        const err = { error: "Failed to list MCP tools.", details: String(e) };
        set_result(err);
        print(JSON.stringify(err, null, 2));
        return;
    }

    if (!toolName) {
        const err = { error: "No convert_to_markdown tool found in MCP server." };
        set_result(err);
        print(JSON.stringify(err, null, 2));
        return;
    }

    let resultRaw = "";
    try {
        resultRaw = await tools.call(toolName, { uri });
    } catch (e) {
        const err = { error: "MCP tool call failed.", details: String(e) };
        set_result(err);
        print(JSON.stringify(err, null, 2));
        return;
    }

    try {
        const result = JSON.parse(resultRaw);
        let markdown = "";
        if (result && typeof result.markdown === "string") {
            markdown = result.markdown;
        } else if (result && Array.isArray(result.content)) {
            markdown = result.content
                .filter((item) => item && item.type === "text" && typeof item.text === "string")
                .map((item) => item.text)
                .join("\n");
        }

        if (!markdown) {
            const err = { error: "No markdown content found in MCP response.", raw: result };
            set_result(err);
            print(JSON.stringify(err, null, 2));
            return;
        }

        const prompt = buildStructuringPrompt(markdown);
        let structuredText = "";
        try {
            structuredText = await llm.chat(prompt);
        } catch (e) {
            const err = { error: "llm.chat failed.", details: String(e) };
            set_result(err);
            print(JSON.stringify(err, null, 2));
            return;
        }

        try {
            const structured = JSON.parse(structuredText);
            set_result(structured);
            print(JSON.stringify(structured, null, 2));
        } catch (e) {
            const err = {
                error: "Failed to parse LLM JSON.",
                raw: String(structuredText),
                details: String(e)
            };
            set_result(err);
            print(JSON.stringify(err, null, 2));
        }
    } catch (e) {
        const err = { error: "Failed to parse MCP response.", raw: String(resultRaw), details: String(e) };
        set_result(err);
        print(JSON.stringify(err, null, 2));
    }
}
