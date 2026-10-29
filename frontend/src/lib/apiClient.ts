
export class ApiError extends Error {
    constructor(public status: number, public statusText: string, message: string) {
        super(message);
        this.name = "ApiError";
    }
}

const TIMEOUT_MS = 15_000;

function fetchWithTimeout(url: string, options: RequestInit = {}): Promise<Response> {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), TIMEOUT_MS);
    return fetch(url, { ...options, signal: controller.signal }).finally(() =>
        clearTimeout(timer)
    );
}

async function handleResponse<T>(response: Response): Promise<T> {
    if (!response.ok) {
        let errorMessage = `API Request failed: ${response.statusText}`;
        try {
            const errorData = await response.json();
            errorMessage = errorData.detail || errorData.message || errorMessage;
        } catch {
            // Use default message if JSON parsing fails
        }
        throw new ApiError(response.status, response.statusText, errorMessage);
    }

    if (response.status === 204) {
        return {} as T;
    }

    try {
        return await response.json() as T;
    } catch {
        throw new ApiError(response.status, response.statusText, "Failed to parse response JSON");
    }
}

export const apiClient = {
    get: async <T>(url: string): Promise<T> => {
        const response = await fetchWithTimeout(url);
        return handleResponse<T>(response);
    },

    post: async <T>(url: string, data: unknown): Promise<T> => {
        const response = await fetchWithTimeout(url, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(data),
        });
        return handleResponse<T>(response);
    },

    put: async <T>(url: string, data: unknown): Promise<T> => {
        const response = await fetchWithTimeout(url, {
            method: "PUT",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(data),
        });
        return handleResponse<T>(response);
    },

    patch: async <T>(url: string, data: unknown): Promise<T> => {
        const response = await fetchWithTimeout(url, {
            method: "PATCH",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(data),
        });
        return handleResponse<T>(response);
    },

    delete: async <T>(url: string): Promise<T> => {
        const response = await fetchWithTimeout(url, {
            method: "DELETE",
        });
        return handleResponse<T>(response);
    },
};
