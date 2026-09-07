<script setup>
// 底部悬浮 Dock（窄屏）：首页 / 照片库 / 设置（v7 裁定：不放侧边栏入口）
import { useRoute, navigate } from "../../lib/router";
const route = useRoute();
</script>

<template>
  <nav class="bottomnav" aria-label="导航">
    <button class="bn-item" :class="{ on: route === 'home' }" @click="navigate('home')">首页</button>
    <button class="bn-item" :class="{ on: route === 'library' }" @click="navigate('library')">
      照片库
    </button>
    <button class="bn-item" :class="{ on: route === 'settings' }" @click="navigate('settings')">
      设置
    </button>
  </nav>
</template>
