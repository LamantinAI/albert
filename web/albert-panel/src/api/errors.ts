export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
    public param?: string | null,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

export function errorMessage(error: unknown) {
  return error instanceof Error
    ? error.message
    : "The request could not be completed.";
}
