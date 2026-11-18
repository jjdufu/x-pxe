import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

afterEach(() => {
  cleanup();
});

// jsdom 缺少的浏览器 API（Ark 浮层定位与列表滚动会调用）
if (!("ResizeObserver" in globalThis)) {
  class ResizeObserverStub {
    observe(): void {
      // 测试环境无布局
    }
    unobserve(): void {
      // 测试环境无布局
    }
    disconnect(): void {
      // 测试环境无布局
    }
  }
  Object.defineProperty(globalThis, "ResizeObserver", { value: ResizeObserverStub });
}
if (typeof Element.prototype.scrollIntoView !== "function") {
  Element.prototype.scrollIntoView = function scrollIntoView() {
    // 测试环境无滚动
  };
}
if (typeof window.matchMedia !== "function") {
  Object.defineProperty(window, "matchMedia", {
    value: (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      addListener: () => undefined,
      removeListener: () => undefined,
      dispatchEvent: () => false,
    }),
  });
}
if (typeof Element.prototype.scrollTo !== "function") {
  Element.prototype.scrollTo = function scrollTo() {
    // 测试环境无滚动
  };
}
