<script setup lang="ts">
// 悬浮搜索（照片库顶部，UI 对齐 v2/v8）：实时搜 + Ctrl K 聚焦；范围跟随侧栏选择
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import type { AssetSummary } from "@miaomory/contracts";
import { useSearch } from "../../composables/useSearch";
import { useFolders } from "../../composables/useFolders";
import { useWorkspaceSelection } from "../../composables/useWorkspaceSelection";
import { useLightbox } from "../../composables/useLightbox";
import { formatDate } from "../../lib/format";
import PhotoTile from "../PhotoTile.vue";

const search = useSearch();
const { folders } = useFolders();
const { selectedFolderId } = useWorkspaceSelection();
const lightbox = useLightbox();
const inputEl = ref<HTMLInputElement | null>(null);
const panelOpen = ref(false);

const scopeName = computed(() => {
  const f = folders.value.find((f2) => String(f2.folder_id) === selectedFolderId.value);
  return f?.label || f?.path || "";
});
const placeholder = computed(() =>
  scopeName.value ? `在「${scopeName.value}」中搜索` : "搜索照片，如：海边的日落",
);
const hits = computed(() => search.results.value ?? []);

function onKeydown(e: KeyboardEvent) {
  if (e.key === "Escape") {
    panelOpen.value = false;
    inputEl.value?.blur();
  }
}

function onGlobalKey(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
    e.preventDefault();
    inputEl.value?.focus();
  }
}

function onInput() {
  panelOpen.value = true;
  search.run(false);
}

function openHit(item: AssetSummary) {
  const idx = hits.value.findIndex((h) => h.summary.asset_id === item.asset_id);
  lightbox.open(
    hits.value.map((h) => h.summary),
    Math.max(0, idx),
  );
}

function badgeOf(matched: string): string {
  return matched === "semantic" ? "语义" : matched === "text" ? "文件名" : "";
}

function onWindowClick(e: MouseEvent) {
  const el = e.target as HTMLElement;
  if (!el.closest(".search-float")) panelOpen.value = false;
}

onMounted(() => {
  window.addEventListener("keydown", onGlobalKey);
  window.addEventListener("click", onWindowClick);
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onGlobalKey);
  window.removeEventListener("click", onWindowClick);
});
</script>

<template>
  <div class="search-float">
    <div class="searchbox">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.4-3.4" /></svg>
      <input
        ref="inputEl"
        v-model="search.query.value"
        type="text"
        :placeholder="placeholder"
        aria-label="语义搜索照片"
        autocomplete="off"
        @input="onInput"
        @focus="panelOpen = true"
        @keydown="onKeydown"
      />
      <span v-if="search.searching.value" class="st">搜索中…</span>
      <kbd>Ctrl K</kbd>
    </div>

    <div v-if="panelOpen && search.results.value !== null" class="search-panel">
      <div class="ph-head">
        <span>
          找到 <b class="num">{{ hits.length }}</b> 张与「<b>{{ search.query.value.trim() }}</b>」相关的照片
        </span>
        <span v-if="scopeName">· 范围「<b>{{ scopeName }}</b>」</span>
        <span class="sp-view">
          <button :class="{ on: search.spView.value === 'grid' }" aria-label="网格视图" @click="search.spView.value = 'grid'">▦</button>
          <button :class="{ on: search.spView.value === 'list' }" aria-label="列表视图" @click="search.spView.value = 'list'">☰</button>
        </span>
        <span class="num" style="color: var(--mm-muted)">{{ search.elapsedLabel.value }}</span>
      </div>

      <div v-if="search.searchError.value" class="sp-miss">
        <p>语义搜索暂不可用。</p>
      </div>
      <div v-else-if="hits.length === 0" class="sp-miss">
        <p>没有找到相关照片。试试更具体的词。</p>
      </div>
      <div v-else-if="search.spView.value === 'grid'" class="sp-grid">
        <PhotoTile
          v-for="h in hits"
          :key="h.summary.asset_id"
          :item="h.summary"
          :badge="badgeOf(h.matched)"
          @open="openHit"
        />
      </div>
      <div v-else class="sp-list">
        <button
          v-for="h in hits"
          :key="h.summary.asset_id"
          class="sp-row"
          @click="openHit(h.summary)"
        >
          <span class="row-main">
            <span class="row-name">{{ h.file_name }}</span>
            <span class="row-sub num">
              {{ formatDate(h.summary.taken_at) }} · {{ h.folder_label }}
            </span>
          </span>
          <span v-if="badgeOf(h.matched)" class="row-badge">{{ badgeOf(h.matched) }}</span>
        </button>
      </div>
    </div>
  </div>
</template>
