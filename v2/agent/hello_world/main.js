import console from './console.js';

export default async function main(args) {
    print("Hello world");
    try {
        const response = await llm.chat("Hello");
        console.log(response);
    } catch (e) {
        console.log("Error calling LLM: " + e);
    }

    return {
        message: "Hello world"
    };
}