// 主题状态（设置页外观卡 + 侧栏留待后续）：持久化 localStorage，立即生效
import { ref } from "vue";

const THEME_KEY = "miaomory.theme";
const stored =
  typeof localStorage !== "undefined" ? localStorage.getItem(THEME_KEY) : null;
const theme = ref<"light" | "dark">(stored === "dark" ? "dark" : "light");
document.documentElement.dataset.theme = theme.value;

function apply() {
  document.documentElement.dataset.theme = theme.value;
  localStorage.setItem(THEME_KEY, theme.value);
}

export function useTheme() {
  function setTheme(t: "light" | "dark") {
    theme.value = t;
    apply();
  }
  function toggleTheme() {
    setTheme(theme.value === "dark" ? "light" : "dark");
  }
  return { theme, setTheme, toggleTheme };
}
