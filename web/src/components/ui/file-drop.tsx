// 参考实现：.dropzone（拖放或点击选择）。
import { FileUpload } from "@ark-ui/react/file-upload";
import { Upload } from "lucide-react";

const PROMPT = "拖放文件到此处，或点击选择文件";

/** 拖放区本身就是按钮（点击或 Enter/Space 打开文件选择），内部不再嵌套按钮。 */
export function FileDrop({ label, accept, maxBytes, onFiles }: { label: string; accept?: string; maxBytes?: number; onFiles: (files: File[]) => void }) {
  return (
    <FileUpload.Root
      maxFiles={1}
      {...(accept === undefined ? {} : { accept })}
      {...(maxBytes === undefined ? {} : { maxFileSize: maxBytes })}
      onFileAccept={(details) => {
        onFiles(details.files);
      }}
    >
      <FileUpload.Label className="ui-label">{label}</FileUpload.Label>
      <FileUpload.Dropzone className="dropzone" aria-label={`${label}：${PROMPT}`}>
        <Upload size={18} aria-hidden="true" />
        <div>{PROMPT}</div>
      </FileUpload.Dropzone>
      <FileUpload.ItemGroup className="file-list">
        <FileUpload.Context>
          {(context) =>
            context.acceptedFiles.map((file) => (
              <FileUpload.Item key={file.name} file={file} className="file-item">
                <FileUpload.ItemName />
                <FileUpload.ItemSizeText className="muted-text tabular" />
              </FileUpload.Item>
            ))
          }
        </FileUpload.Context>
      </FileUpload.ItemGroup>
      <FileUpload.HiddenInput />
    </FileUpload.Root>
  );
}
