import { Internal, Metadata } from "@fig/autocomplete-shared";
import type { Result } from "@aws/amazon-q-developer-cli-fuzzysort";

export type SpecLocation = Fig.SpecLocation & {
  diffVersionedFile?: string;
};

type Override<T, S> = Omit<T, keyof S> & S;
export type SuggestionType = Fig.SuggestionType | "history" | "auto-execute";
export type Suggestion<ArgT = Metadata.ArgMeta> = Override<
  Fig.Suggestion,
  {
    type?: SuggestionType;
    // Whether or not to add a space after suggestion, e.g. if user completes a
    // subcommand that takes a mandatory arg.
    shouldAddSpace?: boolean;
    // Whether or not to add a separator after suggestion, e.g. for options with requiresSeparator
    separatorToAdd?: string;
    args?: ArgT[];
    // Generator information to determine whether suggestion should be filtered.
    generator?: Fig.Generator;
    getQueryTerm?: (x: string) => string;
    fuzzyMatchData?: (Result | null)[];
    originalType?: SuggestionType;
  }
>;

export type Arg = Metadata.ArgMeta;
export type Option = Internal.Option<Metadata.ArgMeta, Metadata.OptionMeta>;
export type Subcommand = Internal.Subcommand<
  Metadata.ArgMeta,
  Metadata.OptionMeta,
  Metadata.SubcommandMeta
>;
