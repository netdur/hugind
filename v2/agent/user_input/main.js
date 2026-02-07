export default async function main(args) {
    const rawQuery = await input("Enter your prompt: ");
    set_result({
        message: rawQuery.trim()
    });
}
