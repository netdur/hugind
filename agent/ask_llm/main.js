export default async function main(args) {
    const rawQuery = args["message"];
    const response = await llm.chat(rawQuery);
    print(response);
    set_result({ message: response });
}
