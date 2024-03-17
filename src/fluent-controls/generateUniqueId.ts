let counter: number = 0;

export function generateUniqueId(prefix?: string): string {
    counter += 1;
    if (prefix != null) {
        return `${prefix}-id-${counter}`;
    } else {
        return `id-${counter}`;
    }
}
