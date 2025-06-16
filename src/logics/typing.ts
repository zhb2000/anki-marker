/** 
 * 用于在 TypeScript 中进行类型断言。
 * 
 * @param obj 需要进行类型断言的对象。
 * @template T 断言后的类型。
 * @returns 断言 obj 的类型为 T。
 * 
 * @example
 * ```typescript
 * const data: any = response.json();
 * // do some checks on `data`
 * // ...
 * // As long as you are sure `data` is of type `MyInterface`
 * typeAssertion<MyInterface>(data);
 * // Now TypeScript knows that `data` is of type `MyInterface`
 * const data2: MyInterface = data; // No error
 * ```
 * 
 * @remarks 该函数不会执行任何运行时检查，它仅用于类型系统的静态分析。
*/
export function typeAssertion<T>(obj: any): asserts obj is T { }

/** 辅助类型，用于条件类型检查。*/
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
    [P in keyof T]: T[P] extends (...args: any[]) => any ? P : never
}[keyof T];

export type Constructor<T> = new (...args: any[]) => T;
