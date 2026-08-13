import { type } from "arktype";
import { parseAccountRef } from "@/infrastructure/mastodon/parsers/account";

export const parseAccountList = type(parseAccountRef, "[]");
