import type { MessageCatalog } from "../types";

export const EN_API_KEYS_MESSAGES: MessageCatalog = {
  "Gateway access": "Gateway access",
  "内网 IP 用量": "LAN IP usage",
  "客户端 IP": "Client IP",
  项目: "Project",
  请求: "Requests",
  "Token / 金额": "Token / Amount",
  "成功 / 异常": "Success / Errors",
  已花费: "Spent",
  不限额: "Unlimited",
  已达上限: "Limit reached",
  管理员视图: "Admin view",
  成员视图: "Member view",
  "按 Token 排序": "Sort by token",
  "按平台密钥和直接访问的内网 IP 汇总":
    "Summarized by platform key and directly connected LAN IP",
  "搜索路径、账号、密钥 ID 或客户端 IP...":
    "Search path, account, key ID, or client IP...",
  "请选择平台 Key 归属成员": "Select the member owner for this platform key",
  账号组筛选: "Account group filter",
  "额度分发开启时，平台 Key 必须归属到一个成员钱包。":
    "When quota distribution is enabled, the platform key must belong to a member wallet.",
  "未开启额度分发时可先不分配，开启后再补齐归属。":
    "When quota distribution is not enabled, you may leave this unassigned and fill in ownership later.",
  "总额度限制 (Token，可选)": "Total quota limit (tokens, optional)",
  不填表示不限制: "Leave blank for no limit",
  K: "K",
  M: "M",
  "达到上限后，这把平台密钥的新请求会被拒绝；已在途请求会按完成后的真实用量继续统计。":
    "After the limit is reached, new requests using this platform key will be rejected. In-flight requests continue to be counted by their final actual usage.",
  按: "By",
  参考估算: "Reference estimate",
  "暂无内网 IP 用量记录": "No LAN IP usage records yet",
  最近出现: "Last seen",
};
