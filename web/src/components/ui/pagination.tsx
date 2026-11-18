// 参考实现：.pagination/.page-btn。
import { Pagination as ArkPagination } from "@ark-ui/react/pagination";
import { ChevronLeft, ChevronRight } from "lucide-react";

export function Pagination({ count, pageSize, page, onPageChange }: { count: number; pageSize: number; page: number; onPageChange: (page: number) => void }) {
  return (
    <ArkPagination.Root
      className="pagination"
      count={count}
      pageSize={pageSize}
      page={page}
      siblingCount={1}
      onPageChange={(details) => {
        onPageChange(details.page);
      }}
    >
      <ArkPagination.PrevTrigger className="page-btn" aria-label="上一页">
        <ChevronLeft size={14} aria-hidden="true" />
      </ArkPagination.PrevTrigger>
      <ArkPagination.Context>
        {(pagination) =>
          pagination.pages.map((item, index) =>
            item.type === "page" ? (
              <ArkPagination.Item key={`p-${String(item.value)}`} {...item} className="page-btn tabular">
                {item.value}
              </ArkPagination.Item>
            ) : (
              <ArkPagination.Ellipsis key={`e-${String(index)}`} index={index} className="page-ellipsis">
                …
              </ArkPagination.Ellipsis>
            ),
          )
        }
      </ArkPagination.Context>
      <ArkPagination.NextTrigger className="page-btn" aria-label="下一页">
        <ChevronRight size={14} aria-hidden="true" />
      </ArkPagination.NextTrigger>
    </ArkPagination.Root>
  );
}
