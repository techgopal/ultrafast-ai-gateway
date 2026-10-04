import { control, cutLongChoice, selectList } from "@/components/classes";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

/** A select among the filters: no wider than the page. */
const filterTrigger = `${control} max-w-full ${cutLongChoice}`;

export interface Choice {
  value: string;
  label: string;
}

interface FilterSelectProps {
  label: string;
  value: string;
  choices: readonly Choice[];
  onChange: (value: string) => void;
}

export function FilterSelect({ label, value, choices, onChange }: FilterSelectProps) {
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger aria-label={label} className={filterTrigger}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent className={selectList}>
        {choices.map((choice) => (
          <SelectItem key={choice.value} value={choice.value}>
            {choice.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
