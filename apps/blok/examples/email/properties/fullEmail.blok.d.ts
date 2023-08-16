import { PropertyType, Versioned, Object } from '@std/types';
import { Text } from '@std/data';
import { PrimaryEMail } from "./primaryEmail.blok";
import { SecondaryEMail } from "./secondaryEmail.blok";

/**
 * Full E-Mail Address
 *
 * A full e-mail address is a primary e-mail address or a secondary e-mail address.
 *
 * @id full-email
 * @version 1
 */
interface Latest extends PropertyType {
    // Not created yet, will be set to 1 when created.
    version: unknown;

    oneOf:
        | Object<PrimaryEMail & SecondaryEMail>
        | Text;
}

export type FullEMail = Versioned<PropertyType, Latest>;
