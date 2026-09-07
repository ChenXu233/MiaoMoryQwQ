<script setup lang="ts">
// 首页（UI 对齐 v6）：居中 logo + 大搜索框；回车提交 → 滑顶结果态；
// 结果网格/列表带元数据（文件名/日期/文件夹/大小/命中徽标）；范围跟随侧栏选择（v8）。
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import type { AssetSummary } from "@miaomory/contracts";
import { useSearch } from "../composables/useSearch";
import { useModelStatus } from "../composables/useModelStatus";
import { useFolders } from "../composables/useFolders";
import { useWorkspaceSelection } from "../composables/useWorkspaceSelection";
import { useLightbox } from "../composables/useLightbox";
import { formatDate } from "../lib/format";
import { assetSrc } from "../lib/ui";
import PhotoTile from "../components/PhotoTile.vue";

const search = useSearch();
const model = useModelStatus();
const { folders } = useFolders();
const { selectedFolderId } = useWorkspaceSelection();
const lightbox = useLightbox();

const searched = ref(false);
const inputEl = ref<HTMLInputElement | null>(null);

const scopeName = computed(() => {
  const f = folders.value.find((f2) => String(f2.folder_id) === selectedFolderId.value);
  return f?.label || f?.path || "";
});
const placeholder = computed(() =>
  scopeName.value
    ? `在「${scopeName.value}」中搜索，回车开始`
    : "搜索你的照片，回车开始",
);
const hits = computed(() => search.results.value ?? []);
const hasQuery = computed(() => search.query.value.trim().length > 0);

function commit() {
  if (!hasQuery.value) return;
  searched.value = true;
  void search.run(true);
}

function reset() {
  search.clear();
  searched.value = false;
}

function onGlobalKey(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
    e.preventDefault();
    inputEl.value?.focus();
  }
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === "Enter") commit();
  else if (e.key === "Escape") {
    reset();
    inputEl.value?.blur();
  }
}

function openHit(item: AssetSummary) {
  const idx = hits.value.findIndex((h) => h.summary.asset_id === item.asset_id);
  lightbox.open(
    hits.value.map((h) => h.summary),
    Math.max(0, idx),
  );
}

onMounted(() => window.addEventListener("keydown", onGlobalKey));
onBeforeUnmount(() => window.removeEventListener("keydown", onGlobalKey));

function badgeOf(matched: string): string {
  return matched === "semantic" ? "语义" : matched === "text" ? "文件名" : "";
}
</script>

<template>
  <div class="page">
    <div class="home-stage" :class="{ searched }">
      <div class="hero-logo">
        <svg style="width: 34px; height: 34px" viewBox="0 0 24 24" fill="currentColor"><ellipse cx="12" cy="15.6" rx="5.4" ry="4.5" /><circle cx="5.2" cy="10" r="2.15" /><circle cx="9.5" cy="6.9" r="2.25" /><circle cx="14.5" cy="6.9" r="2.25" /><circle cx="18.8" cy="10" r="2.15" /></svg>
      </div>
      <div class="hero">
        <div class="searchbox">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.4-3.4" /></svg>
          <input
            ref="inputEl"
            v-model="search.query.value"
            type="text"
            :placeholder="placeholder"
            aria-label="语义搜索照片"
            autocomplete="off"
            @keydown="onKeydown"
            @input="search.run(false)"
          />
          <span v-if="search.searching.value" class="st">搜索中…</span>
          <kbd>Enter</kbd>
        </div>

        <!-- 结果区（回车后出现） -->
        <div v-if="searched && search.results.value !== null" class="home-results">
          <div class="ph-head">
            <span>
              找到 <b class="num">{{ hits.length }}</b> 张与「<b>{{ search.query.value.trim() }}</b>」相关的照片
            </span>
            <span v-if="scopeName">· 范围「<b>{{ scopeName }}</b>」</span>
            <span class="sp-view">
              <button :class="{ on: search.spView.value === 'grid' }" aria-label="网格视图" title="网格" @click="search.spView.value = 'grid'">▦</button>
              <button :class="{ on: search.spView.value === 'list' }" aria-label="列表视图" title="列表" @click="search.spView.value = 'list'">☰</button>
            </span>
            <span class="num" style="color: var(--mm-muted)">{{ search.elapsedLabel.value }}</span>
          </div>

          <div v-if="!model.modelReady.value" class="sp-miss">
            <p>语义模型正在准备（下载/建索引中），稍等片刻再试。</p>
          </div>
          <div v-else-if="search.searchError.value" class="sp-miss">
            <p>语义搜索暂不可用。</p>
          </div>
          <div v-else-if="hits.length === 0" class="sp-miss">
            <p>没有找到相关照片。试试更具体的词，比如「火锅」「雪山」。</p>
          </div>

          <!-- 网格：元数据在下方（v6） -->
          <div v-else-if="search.spView.value === 'grid'" class="res-grid">
            <div v-for="h in hits" :key="h.summary.asset_id" class="cell">
              <PhotoTile :item="h.summary" :badge="badgeOf(h.matched)" @open="openHit" />
              <span class="cell-name">
                <span class="nm">{{ h.file_name }}</span>
              </span>
              <span class="cell-meta num">
                {{ formatDate(h.summary.taken_at) }} · {{ h.folder_label }}
              </span>
            </div>
          </div>

          <!-- 列表：文件名 + 日期 + 文件夹 + 大小 + 徽标（v6） -->
          <div v-else class="sp-list">
            <button
              v-for="h in hits"
              :key="h.summary.asset_id"
              class="sp-row"
              @click="openHit(h.summary)"
            >
              <span class="row-ph">
                <img v-if="h.summary.thumb_path" :src="assetSrc(h.summary.thumb_path)" alt="" loading="lazy" />
              </span>
              <span class="row-main">
                <span class="row-name">{{ h.file_name }}</span>
                <span class="row-sub num">
                  {{ formatDate(h.summary.taken_at) }} · {{ h.folder_label }} ·
                  {{ (h.size_bytes ?? 0) > 0 ? ((h.size_bytes ?? 0) / 1024 / 1024).toFixed(1) + " MB" : "" }}
                </span>
              </span>
              <span v-if="badgeOf(h.matched)" class="row-badge">{{ badgeOf(h.matched) }}</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
