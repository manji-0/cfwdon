/** UI-layer draft attachment before upload (keeps browser `File` out of domain). */
export type DraftMediaAttachment = Readonly<{
  localId: string;
  file: File;
  previewUrl: string;
}>;
