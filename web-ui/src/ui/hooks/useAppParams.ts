import { useParams } from "@tanstack/react-router";

export const useAppParams = () => useParams({ strict: false });
