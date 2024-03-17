export function typeAssertion<T>(_value: any): asserts _value is T { }

// 辅助类型，用于条件类型检查
type IfEquals<X, Y, A = X, B = never> =
    (<T>() => T extends X ? 1 : 2) extends
    (<T>() => T extends Y ? 1 : 2) ? A : B;

/** The keys of a type that are writable. */
export type WritableKeys<T> = {
    [P in keyof T]: IfEquals<
        { [Q in P]: T[P] },
        { -readonly [Q in P]: T[P] },
        P
    >
}[keyof T];

/** The keys of a type that are methods. */
export type MethodKeys<T> = {
    [P in keyof T]: T[P] extends Function ? P : never
}[keyof T];

export type Constructor<T> = new (...args: any[]) => T;
