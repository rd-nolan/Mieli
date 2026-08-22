import { editorViewOptionsCtx } from "@milkdown/core";
import clsx from "clsx";
//#region src/index.ts
function nord(ctx) {
	ctx.update(editorViewOptionsCtx, (prev) => {
		const prevClass = prev.attributes;
		return {
			...prev,
			attributes: (state) => {
				const attrs = typeof prevClass === "function" ? prevClass(state) : prevClass;
				return {
					...attrs,
					class: clsx("prose dark:prose-invert", attrs?.class || "", "milkdown-theme-nord")
				};
			}
		};
	});
}
//#endregion
export { nord };

//# sourceMappingURL=index.js.map