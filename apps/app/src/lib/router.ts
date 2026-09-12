// 极简 hash 路由（UI 对齐 v2：#/home #/library #/settings；不引 vue-router，零依赖）
import { ref } from "vue";

export type Route = "home" | "library" | "settings";
const ROUTES: readonly Route[] = ["home", "library", "settings"];

function parse(): Route {
  const h = location.hash.replace(/^#\/?/, "");
  // 设置页分区深链（#settings/index 等，SettingsPage 一次性解析）同样落到 settings 路由
  if (h === "settings" || h.startsWith("settings/")) return "settings";
  return (ROUTES as readonly string[]).includes(h) ? (h as Route) : "library";
}

const current = ref<Route>(parse());

window.addEventListener("hashchange", () => {
  current.value = parse();
  // v9：路由切换自动收起窄屏抽屉
  document.body.classList.remove("drawer");
});

export function useRoute() {
  return current;
}

export function navigate(route: Route) {
  if (current.value === route) return;
  location.hash = `#/${route}`;
}
