import { onMount, type JSX } from "solid-js";
import { Icon } from "./Icon";
export function Dialog(props: {
  title: string;
  class?: string;
  close: () => void;
  children: JSX.Element;
}) {
  let dialog!: HTMLDialogElement;
  onMount(() => dialog.showModal());
  return (
    <dialog
      ref={dialog}
      class={`dialog ${props.class ?? ""}`}
      aria-label={props.title}
      onCancel={(e) => {
        e.preventDefault();
        props.close();
      }}
    >
      <button
        type="button"
        class="icon-button dialog-close"
        aria-label="Close dialog"
        onClick={props.close}
      >
        <Icon name="close" />
      </button>
      {props.children}
    </dialog>
  );
}
