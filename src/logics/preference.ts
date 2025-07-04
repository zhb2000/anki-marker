/**
 * 保存数据到 localStorage
 * @param key - 存储的键
 * @param value - 要存储的值 (会自动进行 JSON.stringify)
 */
export function set<T>(key: string, value: T) {
    try {
        const serializedValue = JSON.stringify(value);
        localStorage.setItem(key, serializedValue);
    } catch (error) {
        console.error(`Error saving to localStorage: ${key}`, error);
    }
}

/**
 * 从 localStorage 读取数据
 * @param key - 读取的键
 * @returns 解析后的数据，如果键不存在或解析失败则返回 null
 */
export function get<T>(key: string): T | null {
    try {
        const serializedValue = localStorage.getItem(key);
        if (serializedValue === null) {
            return null;
        }
        return JSON.parse(serializedValue) as T;
    } catch (error) {
        console.error(`Error reading from localStorage: ${key}`, error);
        return null;
    }
}

/**
 * 从 localStorage 移除数据
 * @param key - 要移除的键
 */
export function remove(key: string) {
    try {
        localStorage.removeItem(key);
    } catch (error) {
        console.error(`Error removing from localStorage: ${key}`, error);
    }
}
