import type { CardStatus } from '../../components/CardStatus';
import type { StatusNotice } from '../../fluent-controls/status';
import type * as dict from '../../logics/dict';

/** 主界面的三本词典 */
export type DictName = 'collins' | 'oxford' | 'youdao';

/** 单词列表的单词卡片 */
export interface ItemModel<T> {
    item: T;
    status: CardStatus;
    id: number | null;
    /** 该卡片自己的就地反馈（连接进度、编辑降级提示等） */
    notice?: StatusNotice | null;
}

/** 每本字典中的搜索结果 */
export interface WordItems {
    collins: ItemModel<dict.CollinsItem>[];
    oxford: ItemModel<dict.OxfordItem>[];
    youdao: ItemModel<dict.YoudaoItem>[];
}

/** 划词面板的词元 */
export interface SentenceToken {
    token: string;
}

/**
 * 句子面板的选区：端点都是单词词元下标（含首尾），区间恒为一段连续词元。
 * null 表示没有选中任何词（此时没有查询目标）。
 *
 * 用“一段区间”而不是“一组被标记的词元”表达选中：不连续的选中在类型上就不存在，
 * 不需要在交互或提示里另行约束（见 src/components/SentencePanel.vue 的说明）
 */
export interface TokenSelection {
    start: number;
    end: number;
}
