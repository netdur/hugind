import console from './console.js';

export default async function main(args) {
    print("Hello world");
    try {
        const response = await llm.chat("Hello world!");
        console.log("LLM Response:");
        console.log(response);
    } catch (e) {
        console.log("Error calling LLM: " + e);
    }

    set_result({
        message: "Hello world"
    });
}
