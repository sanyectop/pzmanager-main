import assert from "node:assert/strict";
import test from "node:test";
import { paginationPageNumbers } from "../src/paginationUi.ts";

test("shows first, last, current and its neighbours", () => {
  assert.deepEqual(paginationPageNumbers(5, 10), [1, 4, 5, 6, 10]);
  // 当前页在开头 / 结尾时不显示越界页码
  assert.deepEqual(paginationPageNumbers(1, 10), [1, 2, 10]);
  assert.deepEqual(paginationPageNumbers(10, 10), [1, 9, 10]);
  // 页数很少时去重后就是全部页
  assert.deepEqual(paginationPageNumbers(2, 3), [1, 2, 3]);
  assert.deepEqual(paginationPageNumbers(1, 1), [1]);
});

test("clamps out-of-range input and tolerates garbage", () => {
  assert.deepEqual(paginationPageNumbers(0, 5), [1, 2, 5]);
  assert.deepEqual(paginationPageNumbers(99, 5), [1, 4, 5]);
  assert.deepEqual(paginationPageNumbers(Number.NaN, 5), []);
  assert.deepEqual(paginationPageNumbers(2, 0), []);
});
