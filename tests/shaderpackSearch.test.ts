import assert from "node:assert/strict";
import test from "node:test";
import { shaderpackSearchQueries } from "../src/shaderpackSearch.ts";

test("drops words whose front is glued to a digit", () => {
  // "0a" 里的 a、版本号 v1：整词作废，不能拆出 "a" 这种垃圾单词去搜封面
  assert.deepEqual(shaderpackSearchQueries("photon_v1.0a.zip"), ["photon"]);
});

test("drops digit-prefixed tokens like 3D", () => {
  // "3D" 旧逻辑会拆出垃圾单词 "D"，现在整词作废；整名搜索分支保留原文件名
  assert.deepEqual(shaderpackSearchQueries("3D.zip"), ["3D"]);
});

test("keeps the full file name when it has no version token", () => {
  // "3dskinlayers" 本身就是资源名，整名搜索保留；混排词不进单词列表
  assert.deepEqual(shaderpackSearchQueries("3dskinlayers.zip"), ["3dskinlayers"]);
});

test("splits camel case and drops version tokens", () => {
  assert.deepEqual(shaderpackSearchQueries("EclipseShaders.zip"), [
    "EclipseShaders",
    "Eclipse Shaders",
    "Eclipse",
    "Shaders",
  ]);
  assert.deepEqual(shaderpackSearchQueries("Solas Shader V3.8.zip"), [
    "Solas Shader",
    "Solas",
    "Shader",
  ]);
});

test("separate version numbers do not void neighboring words", () => {
  // 数字独立成段（2 / 13）不算「词前贴数字」，正常词保留
  const queries = shaderpackSearchQueries("Bliss 2.1.2 (Chocapic 13 Shader).zip");
  assert.ok(queries.includes("Bliss Chocapic Shader"));
  assert.ok(queries.includes("Bliss"));
  assert.ok(!queries.includes("13"));
  assert.ok(!queries.includes("2"));
});

test("returns empty for blank names", () => {
  assert.deepEqual(shaderpackSearchQueries(".zip"), []);
  assert.deepEqual(shaderpackSearchQueries("   .zip"), []);
});
