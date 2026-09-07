// 工作区选择（侧栏文件夹选中态）：库页过滤 + 搜索范围的单一事实源（v8 裁定）
import { ref } from "vue";

/** 空串 = 全部照片（跨工作区浏览/搜索） */
export const selectedFolderId = ref<string>("");

export function useWorkspaceSelection() {
  function selectFolder(id: string) {
    selectedFolderId.value = id;
  }
  return { selectedFolderId, selectFolder };
}
