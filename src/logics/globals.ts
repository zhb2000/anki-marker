import { reactive } from 'vue';

import { Config } from './config';

let globalConfig: Config | null = null;

export async function getConfig(): Promise<Config> {
    if (globalConfig == null) {
        const rawConfig = await Config.load();
        globalConfig = reactive(rawConfig) as Config;
    }
    return globalConfig;
}
