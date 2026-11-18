// 参考实现：.accordion-item/.accordion-trigger/.accordion-body。
import { Accordion as ArkAccordion } from "@ark-ui/react/accordion";
import { ChevronDown } from "lucide-react";
import type { ReactNode } from "react";

export interface AccordionSection {
  value: string;
  title: string;
  content: ReactNode;
}

export function Accordion({ sections, defaultValue = [] }: { sections: readonly AccordionSection[]; defaultValue?: string[] }) {
  return (
    <ArkAccordion.Root multiple defaultValue={defaultValue} className="accordion">
      {sections.map((section) => (
        <ArkAccordion.Item key={section.value} value={section.value} className="accordion-item">
          <ArkAccordion.ItemTrigger className="accordion-trigger">
            {section.title}
            <ArkAccordion.ItemIndicator className="accordion-indicator">
              <ChevronDown size={14} aria-hidden="true" />
            </ArkAccordion.ItemIndicator>
          </ArkAccordion.ItemTrigger>
          <ArkAccordion.ItemContent className="accordion-body">{section.content}</ArkAccordion.ItemContent>
        </ArkAccordion.Item>
      ))}
    </ArkAccordion.Root>
  );
}
