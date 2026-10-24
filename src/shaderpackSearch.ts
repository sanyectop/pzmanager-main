/** 已安装光影包的封面搜索词生成：把文件名拆成候选查询词，供资源中心搜封面图标。
 *
 * 版本号碎片（v1 / V3.8 / 0a 这类字母数字混排）会在 Modrinth / CurseForge 上
 * 命中不相干的结果，整词作废；英文词前面直接贴着数字的（0a / 3D）同样作废，
 * 否则会拆出 "a"、"D" 这种垃圾单词去搜索。 */
export function shaderpackSearchQueries(fileName: string): string[] {
  const fullName = fileName.replace(/\.zip$/i, "").trim();
  if (!fullName) return [];
  const isAsciiName = /^[A-Za-z0-9]+(?:[ _.-][A-Za-z0-9]+)*$/.test(fullName);
  const tokenizedName = fullName
    .replace(/([A-Z]+)([A-Z][a-z])/g, "$1 $2")
    .replace(/([a-z])([A-Z])/g, "$1 $2");
  const words = [...new Set(
    tokenizedName
      .split(/[^A-Za-z0-9]+/)
      .filter((token) => token.length > 0 && !/[A-Za-z]\d/.test(token) && !/\d[A-Za-z]/.test(token))
      .map((token) => token.match(/[A-Za-z]+/g)?.join("") ?? "")
      .filter((token) => token.length > 0)
  )];
  const hasVersionToken = tokenizedName.split(/[^A-Za-z0-9]+/).some((token) => /[A-Za-z]\d/.test(token));
  const queries = isAsciiName && !hasVersionToken ? [fullName] : [];
  if (words.length > 1) queries.push(words.slice(0, 3).join(" "));
  queries.push(...words);
  return [...new Set(queries)].slice(0, 6);
}
