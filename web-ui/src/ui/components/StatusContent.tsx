import { linkifyMentionsInHtml } from "@/ui/lib/linkify-mentions";

type StatusContentProps = Readonly<{
  html: string;
}>;

export const StatusContent = ({ html }: StatusContentProps) => (
  <div className="status-content" dangerouslySetInnerHTML={{ __html: linkifyMentionsInHtml(html) }} />
);
