export type PreviewCardKind = "Link" | "Photo" | "Video" | "Rich" | "Unknown";

export type PreviewCard = Readonly<{
  kind: PreviewCardKind;
  url: string;
  title: string;
  description: string;
  providerName: string;
  providerUrl: string;
  image: string | null;
  blurhash: string | null;
}>;

export const PreviewCard = {
  fromApi: (value: string): PreviewCardKind => {
    switch (value) {
      case "link":
        return "Link";
      case "photo":
        return "Photo";
      case "video":
        return "Video";
      case "rich":
        return "Rich";
      default:
        return "Unknown";
    }
  },
} as const;
