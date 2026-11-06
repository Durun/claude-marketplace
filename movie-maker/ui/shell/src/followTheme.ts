const THEME_PARAM = "theme";

type ThemeName = "light" | "dark";

/** ?theme=dark で固定できる（画面の確認用）。 */
const forcedTheme = (search: string): ThemeName | null => {
  const forced = new URLSearchParams(search).get(THEME_PARAM);
  return forced === "light" || forced === "dark" ? forced : null;
};

/** 指定が無ければ OS の配色に合わせ、OS が切り替わったら追従する。 */
export const followTheme = (root: HTMLElement, search: string, media: typeof matchMedia): void => {
  const apply = (theme: ThemeName) => {
    root.dataset.theme = theme;
  };
  const forced = forcedTheme(search);
  if (forced) return apply(forced);
  const query = media("(prefers-color-scheme: dark)");
  const fromQuery = () => apply(query.matches ? "dark" : "light");
  fromQuery();
  query.addEventListener("change", fromQuery);
};
