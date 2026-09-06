import type { Relationship } from "@/domain/account/relationship";
import { type } from "arktype";
import { mastodon } from "@/infrastructure/mastodon/parsers/definitions";

export const parseRelationship = mastodon.type("RelationshipApi").pipe(
  (value): Relationship => ({
    id: value.id,
    following: value.following,
    followedBy: value.followed_by,
    blocking: value.blocking,
    muting: value.muting,
    requested: value.requested,
    requestedBy: value.requested_by,
    showingReblogs: value.showing_reblogs ?? true,
    notifying: value.notifying ?? false,
  }),
);

export const parseRelationshipList = type(parseRelationship, "[]");
