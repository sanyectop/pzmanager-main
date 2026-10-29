// 显式带 .ts：这个模块会被 node --test 直接 import，省略扩展名在 ESM 下解析不到。
import { steamWorkshopFallbackUrl, steamWorkshopUrl } from "./externalLinks.ts";

export type ModWorkshopLink = {
  /** Steam 协议地址：能唤起客户端时优先用它。 */
  url: string;
  /** 客户端唤不起来时回退到浏览器用的 https 地址。 */
  fallbackUrl: string;
};

/**
 * 由模组自带的创意工坊 ID 拼出工坊页面地址。
 * Lua 模组的 ID 取自创意工坊内容目录名（字符串），JAR 模组取自
 * mod.info 所在路径（数字），两者都会流到这里，所以统一接受两种类型。
 * 本地安装的模组没有创意工坊 ID，返回 null（菜单项置灰）。
 */
export function modWorkshopLink(
  workshopId: string | number | null | undefined,
): ModWorkshopLink | null {
  if (workshopId === null || workshopId === undefined) {
    return null;
  }
  const id = String(workshopId).trim();
  // Steam 的 published file id 一定是正整数；空串、0 和脏数据都不该拼进链接。
  if (!/^[1-9]\d*$/.test(id)) {
    return null;
  }
  return {
    url: steamWorkshopUrl(id),
    fallbackUrl: steamWorkshopFallbackUrl(id),
  };
}
