//! 生成 `packages/contracts/src/bindings.ts`。
//!
//! 用途：本地重新生成契约 + CI 契约一致性检查（生成后 `git diff --exit-code`）。
//! 运行：`cargo run -p miaomory-app --bin export_contracts`

fn main() {
    let builder = miaomory_lib::app_builder();
    miaomory_lib::export_bindings(&builder);
    println!("契约已生成：packages/contracts/src/bindings.ts");
}
