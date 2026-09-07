// 来源文件夹（工作区）列表：状态点 + 重检 + 重指（P5 切片 A1）
import { ref } from "vue";
import { commands, events, type FolderInfo } from "@miaomory/contracts";

const folders = ref<FolderInfo[]>([]);
let started = false;

export function useFolders() {
  if (!started) {
    started = true;
    void events.folderStatusChangedEvent.listen(() => void load());
  }
  async function load() {
    const res = await commands.listFolders();
    if (res.status === "ok") folders.value = res.data;
  }
  void load();
  async function recheck(folderId: number): Promise<FolderInfo | null> {
    const res = await commands.recheckFolder(folderId);
    void load();
    return res.status === "ok" ? res.data : null;
  }
  async function relocate(folderId: number, newPath: string): Promise<FolderInfo | string> {
    const res = await commands.relocateFolder(folderId, newPath);
    void load();
    return res.status === "ok" ? res.data : res.error;
  }
  return { folders, load, recheck, relocate };
}
