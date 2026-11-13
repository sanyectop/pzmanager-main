/** 统一的翻页条：上一页 / 数字页码 / 下一页，右对齐放在列表右下角。
 *
 *  数字页码沿用创意工坊原来的规则（首末页 + 当前页及前后各一页），
 *  创意工坊列表与光影包下载列表共用同一套外观。 */
import { Button, HStack } from "@chakra-ui/react";
import { paginationPageNumbers } from "../paginationUi";

export function Pagination({
  page,
  totalPages,
  onChange,
}: {
  page: number;
  totalPages: number;
  onChange: (page: number) => void;
}) {
  if (totalPages <= 1) {
    return null;
  }
  return (
    <HStack className="pz-pagination" gap="1" justify="flex-end" mt="3" wrap="wrap">
      <Button
        disabled={page <= 1}
        onClick={() => onChange(page - 1)}
        size="sm"
        variant="outline"
      >
        上一页
      </Button>
      {paginationPageNumbers(page, totalPages).map((target) => (
        <Button
          key={target}
          colorPalette={target === page ? "blue" : "gray"}
          onClick={() => onChange(target)}
          size="sm"
          variant={target === page ? "solid" : "outline"}
        >
          {target}
        </Button>
      ))}
      <Button
        disabled={page >= totalPages}
        onClick={() => onChange(page + 1)}
        size="sm"
        variant="outline"
      >
        下一页
      </Button>
    </HStack>
  );
}
