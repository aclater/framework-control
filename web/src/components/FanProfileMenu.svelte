<script lang="ts">
    import Icon from "@iconify/svelte";
    import { tick } from "svelte";
    import type {
        FanControlConfig,
        FanProfile,
        FanProfileAutoSwitch,
    } from "../api";

    export let fan: FanControlConfig | undefined;
    /** Flushes pending edits, applies the patch, and reloads the editor if the active profile changed. */
    export let update: (patch: FanControlConfig) => Promise<void>;

    const NAME_MAX = 24;
    const SOURCES: {
        key: keyof FanProfileAutoSwitch;
        label: string;
        icon: string;
    }[] = [
        { key: "ac", label: "Plugged in", icon: "mdi:power-plug-outline" },
        { key: "battery", label: "On battery", icon: "mdi:battery-outline" },
    ];

    let isOpen = false;
    let rootEl: HTMLDivElement;
    let renamingId: string | null = null;
    let renameValue = "";
    let renameInput: HTMLInputElement;
    let confirmDeleteId: string | null = null;
    let alignEnd = true;

    $: profiles = fan?.profiles ?? [];
    $: active = profiles.find((p) => p.id === fan?.active_profile);
    $: auto = fan?.auto_switch ?? {};
    $: activeSource = SOURCES.find((s) => active && auto[s.key] === active.id);

    function close() {
        isOpen = false;
        renamingId = null;
        confirmDeleteId = null;
    }

    function open() {
        // Right-align by default; flip left when the menu would overflow the left edge (narrow screens).
        const MENU_WIDTH = 256;
        alignEnd = rootEl.getBoundingClientRect().right - MENU_WIDTH >= 0;
        isOpen = true;
    }

    function onWindowClick(e: MouseEvent) {
        if (isOpen && !e.composedPath().includes(rootEl)) close();
    }

    function select(id: string) {
        close();
        if (id !== fan?.active_profile) update({ active_profile: id });
    }

    function nextName(): string {
        const names = new Set(profiles.map((p) => p.name));
        let n = profiles.length + 1;
        while (names.has(`Profile ${n}`)) n++;
        return `Profile ${n}`;
    }

    async function createProfile() {
        if (!fan) return;
        const profile: FanProfile = {
            id: crypto.randomUUID().slice(0, 8),
            name: nextName(),
            mode: fan.mode,
            manual: fan.manual,
            curve: fan.curve,
            overrides: fan.overrides,
        };
        await update({
            profiles: [...profiles, profile],
            active_profile: profile.id,
        });
        startRename(profile);
    }

    async function startRename(p: FanProfile) {
        confirmDeleteId = null;
        renamingId = p.id;
        renameValue = p.name;
        await tick();
        renameInput?.select();
    }

    function commitRename() {
        const id = renamingId;
        const name = renameValue.trim().slice(0, NAME_MAX);
        renamingId = null;
        const current = profiles.find((p) => p.id === id);
        if (!current || !name || name === current.name) return;
        update({
            profiles: profiles.map((p) => (p.id === id ? { ...p, name } : p)),
        });
    }

    function onRenameKeydown(e: KeyboardEvent) {
        if (e.key === "Enter") commitRename();
        if (e.key === "Escape") {
            e.stopPropagation();
            renamingId = null;
        }
    }

    function deleteProfile(id: string) {
        confirmDeleteId = null;
        update({ profiles: profiles.filter((p) => p.id !== id) });
    }

    function setAuto(key: keyof FanProfileAutoSwitch, id: string) {
        update({ auto_switch: { ...auto, [key]: id || undefined } });
    }

    function sourcesFor(id: string) {
        return SOURCES.filter((s) => auto[s.key] === id);
    }
</script>

<svelte:window
    on:click={onWindowClick}
    on:keydown={(e) => e.key === "Escape" && isOpen && close()}
/>

<div
    class="dropdown"
    class:dropdown-end={alignEnd}
    class:dropdown-open={isOpen}
    bind:this={rootEl}
>
    <button
        type="button"
        class="btn btn-xs border surface-border gap-1 flex-nowrap max-w-[8rem] font-normal"
        aria-haspopup="menu"
        aria-expanded={isOpen}
        title={active
            ? `Fan profile: ${active.name}${activeSource ? ` (used when ${activeSource.label.toLowerCase()})` : ""}`
            : "Fan profiles"}
        on:click={() => (isOpen ? close() : open())}
    >
        <Icon
            icon={activeSource?.icon ?? "mdi:tune-variant"}
            class="w-3.5 h-3.5 shrink-0 opacity-80"
        />
        <span class="profile-label min-w-0 items-center gap-1">
            <span class="truncate">{active?.name ?? "Profiles"}</span>
            <Icon
                icon="mdi:chevron-down"
                class="w-3 h-3 shrink-0 opacity-70"
            />
        </span>
    </button>

    {#if isOpen}
        <div
            class="dropdown-content z-30 mt-1 p-2 w-64 max-w-[calc(100vw-2rem)] bg-base-100 rounded-box border surface-border shadow-lg flex flex-col gap-1 text-sm"
            role="menu"
        >
            <div class="px-1 pb-1 text-xs opacity-60">Fan profiles</div>
            {#each profiles as p (p.id)}
                <div
                    class="group flex items-center gap-1 rounded-btn hover:bg-base-200"
                    class:bg-base-200={p.id === active?.id}
                >
                    {#if renamingId === p.id}
                        <input
                            class="input input-xs input-bordered flex-1 min-w-0 my-0.5 ml-0.5"
                            maxlength={NAME_MAX}
                            aria-label="Profile name"
                            bind:this={renameInput}
                            bind:value={renameValue}
                            on:keydown={onRenameKeydown}
                            on:blur={commitRename}
                        />
                    {:else if confirmDeleteId === p.id}
                        <span class="flex-1 min-w-0 px-2 py-1 truncate text-xs"
                            >Delete “{p.name}”?</span
                        >
                        <button
                            type="button"
                            class="btn btn-xs btn-error"
                            on:click={() => deleteProfile(p.id)}>Delete</button
                        >
                        <button
                            type="button"
                            class="btn btn-xs btn-ghost"
                            on:click={() => (confirmDeleteId = null)}
                            >Cancel</button
                        >
                    {:else}
                        <button
                            type="button"
                            role="menuitemradio"
                            aria-checked={p.id === active?.id}
                            class="flex-1 min-w-0 flex items-center gap-1.5 px-2 py-1 text-left"
                            on:click={() => select(p.id)}
                        >
                            <Icon
                                icon="mdi:check"
                                class="w-3.5 h-3.5 shrink-0 {p.id ===
                                active?.id
                                    ? ''
                                    : 'invisible'}"
                            />
                            <span class="truncate">{p.name}</span>
                            {#each sourcesFor(p.id) as s (s.key)}
                                <span title="Used when {s.label.toLowerCase()}">
                                    <Icon
                                        icon={s.icon}
                                        class="w-3.5 h-3.5 shrink-0 opacity-60"
                                    />
                                </span>
                            {/each}
                        </button>
                        <button
                            type="button"
                            class="btn btn-ghost btn-xs btn-square opacity-50 hover:opacity-100"
                            aria-label="Rename {p.name}"
                            title="Rename"
                            on:click={() => startRename(p)}
                        >
                            <Icon icon="mdi:pencil-outline" class="w-3.5 h-3.5" />
                        </button>
                        <button
                            type="button"
                            class="btn btn-ghost btn-xs btn-square opacity-50 hover:opacity-100"
                            aria-label="Delete {p.name}"
                            title={profiles.length > 1
                                ? "Delete"
                                : "The last profile can't be deleted"}
                            disabled={profiles.length <= 1}
                            on:click={() => {
                                renamingId = null;
                                confirmDeleteId = p.id;
                            }}
                        >
                            <Icon
                                icon="mdi:trash-can-outline"
                                class="w-3.5 h-3.5"
                            />
                        </button>
                    {/if}
                </div>
            {/each}
            <button
                type="button"
                class="btn btn-ghost btn-xs justify-start gap-1.5 font-normal"
                on:click={createProfile}
            >
                <Icon icon="mdi:plus" class="w-3.5 h-3.5" />
                New profile from current settings
            </button>

            <div class="border-t surface-border mt-1 pt-2 px-1 flex flex-col gap-1.5">
                <div class="text-xs opacity-60">Switch automatically</div>
                {#each SOURCES as s (s.key)}
                    <label class="flex items-center justify-between gap-2">
                        <span class="inline-flex items-center gap-1.5 text-xs">
                            <Icon icon={s.icon} class="w-3.5 h-3.5 opacity-70" />
                            {s.label}
                        </span>
                        <select
                            class="select select-xs select-bordered w-32"
                            value={auto[s.key] ?? ""}
                            on:change={(e) =>
                                setAuto(s.key, e.currentTarget.value)}
                        >
                            <option value="">Don't switch</option>
                            {#each profiles as p (p.id)}
                                <option value={p.id}>{p.name}</option>
                            {/each}
                        </select>
                    </label>
                {/each}
                <p class="text-xs opacity-50 leading-snug">
                    Changes save to the active profile. Picking a profile by
                    hand lasts until the power source changes.
                </p>
            </div>
        </div>
    {/if}
</div>

<style>
    /* Icon only in narrow panels so the "Fan Control" title stays visible; the card is the size container. */
    .profile-label {
        display: none;
    }
    @container (min-width: 31rem) {
        .profile-label {
            display: inline-flex;
        }
    }
</style>
