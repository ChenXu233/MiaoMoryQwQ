<script setup lang="ts">
// 设置页（spec 0006 §3.0 二级分类 IA，2026-09-09 所有者裁定分门别类）：
// 左侧分类导航 + 右侧内容面板；<720px 导航收顶部横排（styles.css 媒体查询）。
// 位置记忆 localStorage["mm-settings-section"]（先例 mm-rail）；默认落点「外观」。
import { onMounted, ref } from "vue";
import AppearanceSection from "../components/settings/AppearanceSection.vue";
import ModelIndexSection from "../components/settings/ModelIndexSection.vue";
import DataSection from "../components/settings/DataSection.vue";

const SECTIONS = [
  { key: "appearance", label: "外观" },
  { key: "index", label: "模型与推理" },
  { key: "data", label: "数据与存储" },
] as const;

type SectionKey = (typeof SECTIONS)[number]["key"];
const STORAGE_KEY = "mm-settings-section";

function loadInitial(): SectionKey {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved && SECTIONS.some((s) => s.key === saved)) return saved as SectionKey;
  } catch {
    /* localStorage 不可用：静默回落默认 */
  }
  return "appearance";
}

const current = ref<SectionKey>(loadInitial());

function select(key: SectionKey) {
  current.value = key;
  try {
    localStorage.setItem(STORAGE_KEY, key);
  } catch {
    /* 忽略写入失败 */
  }
}

onMounted(() => {
  // 深链兜底：hash 形如 #settings/index 时定位分类（不进路由表，仅一次性解析）
  const m = location.hash.match(/^#settings\/([\w-]+)$/);
  if (m && SECTIONS.some((s) => s.key === m[1])) select(m[1] as SectionKey);
});
</script>

<template>
  <div class="page">
    <div class="page-wrap">
      <h1 class="page-title">设置</h1>
      <p class="page-sub">外观即时生效；模型与数据位置在此管理。</p>

      <div class="settings-layout">
        <nav class="settings-nav" aria-label="设置分类">
          <button
            v-for="s in SECTIONS"
            :key="s.key"
            :class="{ on: current === s.key }"
            :aria-current="current === s.key ? 'page' : undefined"
            @click="select(s.key)"
          >
            {{ s.label }}
          </button>
        </nav>

        <div class="settings-panel">
          <AppearanceSection v-if="current === 'appearance'" />
          <ModelIndexSection v-else-if="current === 'index'" />
          <DataSection v-else />
        </div>
      </div>
    </div>
  </div>
</template>
