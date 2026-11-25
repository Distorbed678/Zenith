<script lang="ts">
  import { uiStrings } from '../strings';

  interface Props {
    size?: number;
    decorative?: boolean;
    title?: string;
    class?: string;
  }

  let {
    size = 32,
    decorative = false,
    title = uiStrings.logo.title,
    class: className = ''
  }: Props = $props();
</script>

<svg
  xmlns="http://www.w3.org/2000/svg"
  viewBox="0 0 32 32"
  width={size}
  height={size}
  shape-rendering="crispEdges"
  class="z-sun-logo z-pixelated {className}"
  role={decorative ? undefined : 'img'}
  aria-hidden={decorative ? 'true' : undefined}
  aria-label={decorative ? undefined : title}
>
  {#if !decorative}
    <title>{title}</title>
  {/if}
  <!-- Sun rays -->
  <rect x="16" y="4" width="1" height="2" fill="#FFB938" />
  <rect x="11" y="6" width="1" height="1" fill="#FFB938" />
  <rect x="21" y="6" width="1" height="1" fill="#FFB938" />
  <rect x="9" y="11" width="2" height="1" fill="#FFB938" />
  <rect x="22" y="11" width="2" height="1" fill="#FFB938" />
  <rect x="11" y="16" width="1" height="1" fill="#FFB938" />
  <rect x="21" y="16" width="1" height="1" fill="#FFB938" />
  <rect x="16" y="17" width="1" height="2" fill="#FFB938" />

  <!-- Sun rim (#FF9E2C) -->
  <rect x="15" y="6" width="2" height="1" fill="#FF9E2C" />
  <rect x="13" y="7" width="6" height="1" fill="#FF9E2C" />
  <rect x="12" y="8" width="2" height="1" fill="#FF9E2C" />
  <rect x="18" y="8" width="2" height="1" fill="#FF9E2C" />
  <rect x="12" y="9" width="1" height="1" fill="#FF9E2C" />
  <rect x="19" y="9" width="1" height="1" fill="#FF9E2C" />
  <rect x="11" y="10" width="2" height="2" fill="#FF9E2C" />
  <rect x="19" y="10" width="2" height="2" fill="#FF9E2C" />
  <rect x="12" y="12" width="1" height="1" fill="#FF9E2C" />
  <rect x="19" y="12" width="1" height="1" fill="#FF9E2C" />
  <rect x="12" y="13" width="2" height="1" fill="#FF9E2C" />
  <rect x="18" y="13" width="2" height="1" fill="#FF9E2C" />
  <rect x="13" y="14" width="6" height="1" fill="#FF9E2C" />
  <rect x="15" y="15" width="2" height="1" fill="#FF9E2C" />

  <!-- Sun core (#FFD34D) -->
  <rect x="14" y="8" width="4" height="6" fill="#FFD34D" />
  <rect x="13" y="9" width="6" height="4" fill="#FFD34D" />

  <!-- Dotted zenith arc (#F6E7B8) -->
  <rect x="6" y="13" width="1" height="1" fill="#F6E7B8" />
  <rect x="8" y="13" width="1" height="1" fill="#F6E7B8" />
  <rect x="23" y="13" width="1" height="1" fill="#F6E7B8" />
  <rect x="25" y="13" width="1" height="1" fill="#F6E7B8" />
  <rect x="5" y="14" width="1" height="1" fill="#F6E7B8" />
  <rect x="26" y="14" width="1" height="1" fill="#F6E7B8" />
  <rect x="4" y="16" width="1" height="1" fill="#F6E7B8" />
  <rect x="27" y="16" width="1" height="1" fill="#F6E7B8" />
  <rect x="3" y="17" width="1" height="1" fill="#F6E7B8" />
  <rect x="28" y="17" width="1" height="1" fill="#F6E7B8" />
  <rect x="2" y="18" width="1" height="1" fill="#F6E7B8" />
  <rect x="29" y="18" width="1" height="1" fill="#F6E7B8" />
  <rect x="1" y="20" width="1" height="2" fill="#F6E7B8" />
  <rect x="30" y="20" width="1" height="2" fill="#F6E7B8" />

  <!-- Stepped teal/navy horizon hills -->
  <rect x="14" y="23" width="3" height="1" fill="#5EE0CF" />
  <rect x="5" y="24" width="2" height="1" fill="#5EE0CF" />
  <rect x="13" y="24" width="1" height="1" fill="#5EE0CF" />
  <rect x="14" y="24" width="3" height="1" fill="#2EA3A8" />
  <rect x="17" y="24" width="1" height="1" fill="#5EE0CF" />
  <rect x="24" y="24" width="3" height="1" fill="#5EE0CF" />

  <rect x="2" y="25" width="3" height="1" fill="#5EE0CF" />
  <rect x="5" y="25" width="2" height="1" fill="#2EA3A8" />
  <rect x="7" y="25" width="2" height="1" fill="#5EE0CF" />
  <rect x="12" y="25" width="1" height="1" fill="#5EE0CF" />
  <rect x="13" y="25" width="1" height="1" fill="#2EA3A8" />
  <rect x="14" y="25" width="1" height="1" fill="#284A72" />
  <rect x="15" y="25" width="1" height="1" fill="#1F3A5E" />
  <rect x="16" y="25" width="1" height="1" fill="#284A72" />
  <rect x="17" y="25" width="1" height="1" fill="#2EA3A8" />
  <rect x="18" y="25" width="2" height="1" fill="#5EE0CF" />
  <rect x="22" y="25" width="2" height="1" fill="#5EE0CF" />
  <rect x="24" y="25" width="3" height="1" fill="#2EA3A8" />
  <rect x="27" y="25" width="2" height="1" fill="#5EE0CF" />

  <rect x="0" y="26" width="2" height="1" fill="#5EE0CF" />
  <rect x="2" y="26" width="3" height="1" fill="#2EA3A8" />
  <rect x="5" y="26" width="2" height="1" fill="#284A72" />
  <rect x="7" y="26" width="2" height="1" fill="#2EA3A8" />
  <rect x="9" y="26" width="3" height="1" fill="#5EE0CF" />
  <rect x="12" y="26" width="1" height="1" fill="#2EA3A8" />
  <rect x="13" y="26" width="1" height="1" fill="#284A72" />
  <rect x="14" y="26" width="1" height="1" fill="#1F3A5E" />
  <rect x="15" y="26" width="3" height="1" fill="#284A72" />
  <rect x="18" y="26" width="2" height="1" fill="#2EA3A8" />
  <rect x="20" y="26" width="2" height="1" fill="#5EE0CF" />
  <rect x="22" y="26" width="2" height="1" fill="#2EA3A8" />
  <rect x="24" y="26" width="1" height="1" fill="#1F3A5E" />
  <rect x="25" y="26" width="2" height="1" fill="#284A72" />
  <rect x="27" y="26" width="2" height="1" fill="#2EA3A8" />
  <rect x="29" y="26" width="3" height="1" fill="#5EE0CF" />

  <rect x="0" y="27" width="2" height="1" fill="#2EA3A8" />
  <rect x="2" y="27" width="7" height="1" fill="#284A72" />
  <rect x="3" y="27" width="1" height="1" fill="#1F3A5E" />
  <rect x="8" y="27" width="1" height="1" fill="#1F3A5E" />
  <rect x="9" y="27" width="3" height="1" fill="#2EA3A8" />
  <rect x="12" y="27" width="8" height="1" fill="#284A72" />
  <rect x="13" y="27" width="1" height="1" fill="#1F3A5E" />
  <rect x="18" y="27" width="1" height="1" fill="#1F3A5E" />
  <rect x="20" y="27" width="2" height="1" fill="#2EA3A8" />
  <rect x="22" y="27" width="7" height="1" fill="#284A72" />
  <rect x="23" y="27" width="1" height="1" fill="#1F3A5E" />
  <rect x="28" y="27" width="1" height="1" fill="#1F3A5E" />
  <rect x="29" y="27" width="3" height="1" fill="#2EA3A8" />

  <!-- Lower navy base with subtle diagonal pixel texture -->
  <rect x="0" y="28" width="32" height="4" fill="#284A72" />
  <rect x="2" y="28" width="1" height="1" fill="#1F3A5E" />
  <rect x="7" y="28" width="1" height="1" fill="#1F3A5E" />
  <rect x="12" y="28" width="1" height="1" fill="#1F3A5E" />
  <rect x="17" y="28" width="1" height="1" fill="#1F3A5E" />
  <rect x="22" y="28" width="1" height="1" fill="#1F3A5E" />
  <rect x="27" y="28" width="1" height="1" fill="#1F3A5E" />
  <rect x="1" y="29" width="1" height="1" fill="#1F3A5E" />
  <rect x="6" y="29" width="1" height="1" fill="#1F3A5E" />
  <rect x="11" y="29" width="1" height="1" fill="#1F3A5E" />
  <rect x="16" y="29" width="1" height="1" fill="#1F3A5E" />
  <rect x="21" y="29" width="1" height="1" fill="#1F3A5E" />
  <rect x="26" y="29" width="1" height="1" fill="#1F3A5E" />
  <rect x="31" y="29" width="1" height="1" fill="#1F3A5E" />
  <rect x="0" y="30" width="1" height="1" fill="#1F3A5E" />
  <rect x="5" y="30" width="1" height="1" fill="#1F3A5E" />
  <rect x="10" y="30" width="1" height="1" fill="#1F3A5E" />
  <rect x="15" y="30" width="1" height="1" fill="#1F3A5E" />
  <rect x="20" y="30" width="1" height="1" fill="#1F3A5E" />
  <rect x="25" y="30" width="1" height="1" fill="#1F3A5E" />
  <rect x="30" y="30" width="1" height="1" fill="#1F3A5E" />
  <rect x="4" y="31" width="1" height="1" fill="#1F3A5E" />
  <rect x="9" y="31" width="1" height="1" fill="#1F3A5E" />
  <rect x="14" y="31" width="1" height="1" fill="#1F3A5E" />
  <rect x="19" y="31" width="1" height="1" fill="#1F3A5E" />
  <rect x="24" y="31" width="1" height="1" fill="#1F3A5E" />
  <rect x="29" y="31" width="1" height="1" fill="#1F3A5E" />
</svg>

<style>
  .z-sun-logo {
    flex-shrink: 0;
  }
</style>
