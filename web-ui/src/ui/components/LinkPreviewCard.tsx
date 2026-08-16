import type { PreviewCard } from "@/domain/status/preview-card";

type LinkPreviewCardProps = Readonly<{
  card: PreviewCard;
}>;

export const LinkPreviewCard = ({ card }: LinkPreviewCardProps) => (
  <a
    className="status-link-card"
    href={card.url}
    target="_blank"
    rel="nofollow noopener noreferrer"
  >
    {card.image ? (
      <img className="status-link-card-image" src={card.image} alt="" loading="lazy" />
    ) : null}
    <div className="status-link-card-body">
      {card.providerName ? (
        <span className="status-link-card-provider">{card.providerName}</span>
      ) : null}
      <strong className="status-link-card-title">{card.title}</strong>
      {card.description ? <p className="status-link-card-description">{card.description}</p> : null}
    </div>
  </a>
);
